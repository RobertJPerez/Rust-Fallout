//! Bounded screenshot conversion and publication outside Bevy's update loop.
use bevy::{
    prelude::{Image, Resource},
    render::render_resource::{TextureDimension, TextureFormat},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, TryRecvError},
    },
    thread::{self, JoinHandle},
};

const MAX_DIMENSION: u32 = 8192;
pub const MAX_PIXELS: u64 = 4 * 1024 * 1024;
const MAX_RAW_BYTES: usize = 16 * 1024 * 1024;
const MAX_PNG_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ARTIFACTS: usize = 8;
const MAX_ARTIFACT_BYTES: usize = 4 * 1024 * 1024;
const MAX_PATH_BYTES: usize = 1024;

#[derive(Debug)]
pub struct Artifact {
    pub path: PathBuf,
    pub bytes: Arc<[u8]>,
}

#[derive(Debug)]
pub struct Written {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub png_sha256: String,
}

struct Pending {
    receiver: Mutex<Receiver<Result<Written, String>>>,
    worker: JoinHandle<()>,
}

#[derive(Default)]
struct WriterState {
    pending: Option<Pending>,
    finished: Vec<JoinHandle<()>>,
}

/// Owns the only active image writer. Clones share its bounded state so the
/// outer run function can drain worker handles after Bevy consumes the App.
#[derive(Resource, Default, Clone)]
pub struct Writer {
    state: Arc<Mutex<WriterState>>,
}

impl Writer {
    pub fn start(
        &self,
        image: &Image,
        path: PathBuf,
        artifacts: Vec<Artifact>,
        fixture: Option<(crate::fixture::Report, PathBuf)>,
    ) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.pending.is_some() {
            return Err("A prior screenshot writer still owns the capture slot".into());
        }
        validate_image(image)?;
        validate_path(&path)?;
        validate_artifacts(&artifacts)?;
        if let Some((_, path)) = &fixture {
            validate_path(path)?;
        }

        // ScreenshotCaptured lends its Image. This single bounded clone freezes
        // the readback for the worker; no ECS/App/render handles cross threads.
        let image = image.clone();
        let worker_path = path.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("preview-capture-writer".into())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    write_capture(image, worker_path, artifacts, fixture)
                }))
                .unwrap_or_else(|_| Err("Screenshot writer panicked".into()));
                let result = result.map_err(bounded_error);
                let _ = sender.send(result);
            })
            .map_err(|error| format!("Could not start screenshot writer: {error}"))?;
        state.pending = Some(Pending {
            receiver: Mutex::new(receiver),
            worker,
        });
        Ok(())
    }

    /// Nonblocking. The handle is retained for an out-of-loop join after the
    /// worker has returned and its one terminal result has been received.
    pub fn poll(&self) -> Option<Result<Written, String>> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state
            .pending
            .as_ref()
            .is_some_and(|pending| pending.worker.is_finished())
        {
            return None;
        }
        let mut pending = state.pending.take()?;
        let result = match pending.receiver.get_mut() {
            Ok(receiver) => match receiver.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => {
                    Err("Screenshot writer returned without a result".into())
                }
                Err(TryRecvError::Disconnected) => {
                    Err("Screenshot writer closed its result channel".into())
                }
            },
            Err(_) => Err("Screenshot writer result lock was poisoned".into()),
        };
        state.finished.push(pending.worker);
        Some(result)
    }

    /// Called after `App::run` returns. An unconsumed writer means shutdown
    /// interrupted the capture; any already-written files remain diagnostic.
    pub fn finish(&self) -> Result<(), String> {
        let (pending, finished) = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (state.pending.take(), std::mem::take(&mut state.finished))
        };
        let interrupted = pending.is_some();
        let mut join_failed = false;
        if let Some(pending) = pending {
            join_failed |= pending.worker.join().is_err();
            let receiver = pending
                .receiver
                .into_inner()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let _ = receiver.try_recv();
        }
        for worker in finished {
            join_failed |= worker.join().is_err();
        }
        if interrupted {
            return Err("Application ended before the screenshot result was admitted".into());
        }
        if join_failed {
            return Err("Screenshot writer panicked while draining".into());
        }
        Ok(())
    }
}

pub fn validate_image(image: &Image) -> Result<(u32, u32), String> {
    let descriptor = &image.texture_descriptor;
    let size = descriptor.size;
    if descriptor.dimension != TextureDimension::D2
        || descriptor.sample_count != 1
        || size.depth_or_array_layers != 1
        || size.width == 0
        || size.height == 0
        || size.width > MAX_DIMENSION
        || size.height > MAX_DIMENSION
    {
        return Err(
            "Screenshot dimensions or texture layout exceed the admitted image shape".into(),
        );
    }
    let pixels = u64::from(size.width)
        .checked_mul(u64::from(size.height))
        .filter(|pixels| *pixels <= MAX_PIXELS)
        .ok_or("Screenshot pixel count exceeds its admitted bound")?;
    let bytes_per_pixel = match descriptor.format {
        TextureFormat::R8Unorm => 1usize,
        TextureFormat::Rg8Unorm => 2,
        TextureFormat::Rgba8UnormSrgb
        | TextureFormat::Bgra8UnormSrgb
        | TextureFormat::Bgra8Unorm => 4,
        format => {
            return Err(format!(
                "Screenshot texture format is unsupported: {format:?}"
            ));
        }
    };
    let expected = usize::try_from(pixels)
        .ok()
        .and_then(|pixels| pixels.checked_mul(bytes_per_pixel))
        .filter(|bytes| *bytes <= MAX_RAW_BYTES)
        .ok_or("Screenshot raw pixel buffer exceeds its admitted byte bound")?;
    let actual = image
        .data
        .as_ref()
        .ok_or("Screenshot readback has no initialized pixel buffer")?
        .len();
    if actual != expected {
        return Err(format!(
            "Screenshot pixel extent disagrees with its data buffer: expected {expected}, got {actual}"
        ));
    }
    Ok((size.width, size.height))
}

fn validate_path(path: &std::path::Path) -> Result<(), String> {
    if path.as_os_str().is_empty() || path.as_os_str().to_string_lossy().len() > MAX_PATH_BYTES {
        return Err("Screenshot output path is empty or exceeds its byte bound".into());
    }
    Ok(())
}

fn validate_artifacts(artifacts: &[Artifact]) -> Result<(), String> {
    if artifacts.len() > MAX_ARTIFACTS {
        return Err("Screenshot capture has too many companion artifacts".into());
    }
    let mut total = 0usize;
    for artifact in artifacts {
        validate_path(&artifact.path)?;
        total = total
            .checked_add(artifact.bytes.len())
            .filter(|size| *size <= MAX_ARTIFACT_BYTES)
            .ok_or("Screenshot companion artifacts exceed their byte budget")?;
    }
    Ok(())
}

fn write_capture(
    image: Image,
    path: PathBuf,
    mut artifacts: Vec<Artifact>,
    fixture: Option<(crate::fixture::Report, PathBuf)>,
) -> Result<Written, String> {
    let (width, height) = validate_image(&image)?;
    let dynamic = image
        .try_into_dynamic()
        .map_err(|error| format!("Screenshot pixel conversion failed: {error}"))?;
    let rgb = dynamic.to_rgb8();
    if rgb.dimensions() != (width, height) {
        return Err("Converted screenshot dimensions changed".into());
    }

    let verification = fixture.map(|(mut report, report_path)| {
        let result = report.verify(&rgb);
        let mut bytes = serde_json::to_vec_pretty(&report)
            .map_err(|error| format!("Could not serialize fixture report: {error}"))?;
        bytes.push(b'\n');
        Ok::<_, String>((
            result,
            Artifact {
                path: report_path,
                bytes: Arc::from(bytes),
            },
        ))
    });
    if let Some(verification) = verification {
        let (result, artifact) = verification?;
        artifacts.push(artifact);
        validate_artifacts(&artifacts)?;
        let mut write_result = write_png(&rgb, &path);
        if let Err(error) = write_artifacts(artifacts) {
            write_result = Err(error);
        }
        // Preserve the measured fixture report even when its color checks fail.
        write_result?;
        result.map_err(|error| format!("Synthetic GPU fixture failed: {error}"))?;
    } else {
        write_png(&rgb, &path)?;
        write_artifacts(artifacts)?;
    }

    let png_sha256 = hash_file(&path)?;
    Ok(Written {
        path,
        width,
        height,
        png_sha256,
    })
}

fn write_png(image: &image::RgbImage, path: &std::path::Path) -> Result<(), String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| format!("Could not create screenshot output: {error}"))?;
    let mut output = LimitedFile::new(file, MAX_PNG_BYTES);
    image
        .write_to(&mut output, image::ImageFormat::Png)
        .map_err(|error| format!("Screenshot PNG encoding failed: {error}"))?;
    output
        .file
        .sync_all()
        .map_err(|error| format!("Screenshot PNG sync failed: {error}"))?;
    Ok(())
}

fn write_artifacts(artifacts: Vec<Artifact>) -> Result<(), String> {
    for artifact in artifacts {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&artifact.path)
            .map_err(|error| format!("Could not create screenshot companion artifact: {error}"))?;
        file.write_all(&artifact.bytes)
            .map_err(|error| format!("Screenshot companion write failed: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("Screenshot companion sync failed: {error}"))?;
    }
    Ok(())
}

fn hash_file(path: &std::path::Path) -> Result<String, String> {
    let mut file =
        File::open(path).map_err(|error| format!("Could not hash screenshot: {error}"))?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("Screenshot hash read failed: {error}"))?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

struct LimitedFile {
    file: File,
    position: u64,
    limit: u64,
}

impl LimitedFile {
    fn new(file: File, limit: u64) -> Self {
        Self {
            file,
            position: 0,
            limit,
        }
    }
}

impl Write for LimitedFile {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let end = self
            .position
            .checked_add(bytes.len() as u64)
            .filter(|end| *end <= self.limit)
            .ok_or_else(|| std::io::Error::other("Screenshot PNG exceeds 8 MiB"))?;
        let written = self.file.write(bytes)?;
        self.position += written as u64;
        debug_assert!(self.position <= end);
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

impl Seek for LimitedFile {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let next = self.file.seek(position)?;
        if next > self.limit {
            return Err(std::io::Error::other(
                "Screenshot PNG seek exceeds its byte bound",
            ));
        }
        self.position = next;
        Ok(next)
    }
}

fn bounded_error(error: String) -> String {
    error.chars().take(4096).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{
        asset::RenderAssetUsages,
        render::render_resource::{Extent3d, TextureDimension, TextureFormat},
    };
    use std::{
        fs,
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    fn rgba_image(width: u32, height: u32) -> Image {
        Image::new_fill(
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            &[32, 64, 96, 255],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        )
    }

    #[test]
    fn image_admission_checks_shape_and_exact_readback_extent() {
        let image = rgba_image(2, 3);
        assert_eq!(validate_image(&image).unwrap(), (2, 3));

        let mut truncated = image.clone();
        truncated.data.as_mut().unwrap().pop();
        assert!(validate_image(&truncated).is_err());

        let mut oversized = image;
        oversized.texture_descriptor.size.width = MAX_DIMENSION + 1;
        assert!(validate_image(&oversized).is_err());
    }

    #[test]
    fn worker_writes_png_and_bounded_companion_then_reports_hash() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "fallout-preview-capture-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        let png_path = directory.join("view.png");
        let receipt_path = directory.join("view.json");
        let writer = Writer::default();
        writer
            .start(
                &rgba_image(2, 2),
                png_path.clone(),
                vec![Artifact {
                    path: receipt_path.clone(),
                    bytes: Arc::from(b"{\"ok\":true}\n".to_vec()),
                }],
                None,
            )
            .unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        let written = loop {
            if let Some(result) = writer.poll() {
                break result.unwrap();
            }
            assert!(Instant::now() < deadline, "screenshot writer timed out");
            thread::yield_now();
        };
        writer.finish().unwrap();

        let png = fs::read(&png_path).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().to_rgb8();
        assert_eq!(decoded.dimensions(), (2, 2));
        assert!(decoded.pixels().all(|pixel| pixel.0 == [32, 64, 96]));
        assert_eq!(written.width, 2);
        assert_eq!(written.height, 2);
        assert_eq!(written.png_sha256, format!("{:x}", Sha256::digest(&png)));
        assert_eq!(fs::read(&receipt_path).unwrap(), b"{\"ok\":true}\n");
        fs::remove_dir_all(directory).unwrap();
    }
}
