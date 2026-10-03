//! Start the inspection build from an ignored, machine-local configuration.
//! --check validates the handoff without opening a window or changing game files.
use clap::Parser;
use fallout_data::baseline;
use serde::Deserialize;
use serde_json::json;
use std::{
    error::Error,
    fs::{self, File, OpenOptions},
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
    process::{Command, ExitCode, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Parser)]
#[command(about = "Launch the configured terrain inspection test; gameplay is not implemented")]
struct Options {
    #[arg(long)]
    check: bool,
    #[arg(long)]
    terrain: Option<String>,
    /// Exercise this launcher and the renderer without opening a desktop window.
    #[arg(long, conflicts_with = "check")]
    smoke_test: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema_version: u32,
    install: PathBuf,
    load_order: PathBuf,
    terrain: String,
    texture_repeats_per_quadrant: f32,
}

fn relay(
    mut source: impl Read + Send + 'static,
    log: Arc<Mutex<File>>,
) -> thread::JoinHandle<io::Result<()>> {
    thread::spawn(move || {
        let mut bytes = [0; 4096];
        loop {
            let count = source.read(&mut bytes)?;
            if count == 0 {
                return Ok(());
            }
            // Keep output live without holding the console lock while reading.
            io::stderr().lock().write_all(&bytes[..count])?;
            log.lock()
                .map_err(|_| io::Error::other("Launch log lock failed"))?
                .write_all(&bytes[..count])?;
        }
    })
}

fn run(options: Options) -> Result<()> {
    let executable = std::env::current_exe()?;
    let root = executable
        .ancestors()
        .find(|p| {
            p.join("Cargo.toml").is_file() && p.join("profiles/nv-inspection-order.json").is_file()
        })
        .ok_or("Place this executable under the Rust-Fallout checkout")?;
    let config_path = root.join("local/playtest.json");
    let mut bytes = Vec::new();
    baseline::open_source(&config_path)
        .map_err(|e| {
            format!("Cannot read local/playtest.json: {e}; see docs/terrain-textured-preview.md")
        })?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 {
        return Err("Playtest configuration exceeds 64 KiB".into());
    }
    let config: Config = serde_json::from_slice(&bytes)?;
    let terrain = options.terrain.unwrap_or(config.terrain);
    let repeats = config.texture_repeats_per_quadrant;
    if config.schema_version != 1
        || terrain.is_empty()
        || terrain.len() > 256
        || terrain.chars().any(char::is_control)
        || !repeats.is_finite()
        || repeats <= 0.
        || repeats > 64.
    {
        return Err("Invalid playtest schema, cell selector or texture repetition".into());
    }
    let install = config.install.canonicalize()?;
    let order = root.join(config.load_order).canonicalize()?;
    let preview = executable.with_file_name("fallout-preview.exe");
    if !install.join("Data/FalloutNV.esm").is_file() || !order.is_file() || !preview.is_file() {
        return Err("Playtest needs the original NV data, a load-order file and fallout-preview.exe beside this launcher".into());
    }
    let mut command = Command::new(&preview);
    command
        .current_dir(root)
        .arg("--install")
        .arg(&install)
        .arg("--load-order")
        .arg(&order)
        .arg("--terrain")
        .arg(&terrain)
        .arg("--terrain-textures")
        .arg("--terrain-texture-repeat")
        .arg(repeats.to_string());
    if options.check {
        println!(
            "{}",
            json!({"ready":true,"mode":"terrain inspection; no gameplay","preview":preview,"install":install,
            "load_order":order,"terrain":terrain,"texture_repeats_per_quadrant":repeats,"window_launched":false})
        );
        return Ok(());
    }
    let local = root.join("local").canonicalize()?;
    if !local.starts_with(root.canonicalize()?) || local.starts_with(&install) {
        return Err("Launch logs must stay in this checkout's local directory".into());
    }
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let folder = local.join(format!("playtest-{}-{nonce}", std::process::id()));
    fs::create_dir(&folder)?;
    let log_path = folder.join("startup.log");
    let log = Arc::new(Mutex::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&log_path)?,
    ));
    if options.smoke_test {
        command
            .arg("--headless")
            .arg("--capture")
            .arg(folder.join("capture.png"))
            .arg("--report")
            .arg(folder.join("report.json"));
    }
    println!(
        "Loading {terrain}. {}",
        if options.smoke_test {
            "Running an offscreen startup test."
        } else {
            "The 3D view opens in a separate window; this console shows loading progress."
        }
    );
    println!("Startup log: {}", log_path.display());
    println!(
        "Terrain inspection: Tab toggles orbit/fly; WASD moves; Q/E changes height; arrows turn; R resets; Esc exits."
    );
    io::stdout().flush()?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = relay(
        child.stdout.take().ok_or("Missing viewer stdout")?,
        Arc::clone(&log),
    );
    let stderr = relay(
        child.stderr.take().ok_or("Missing viewer stderr")?,
        Arc::clone(&log),
    );
    let status = child.wait()?;
    stdout.join().map_err(|_| "Viewer output relay failed")??;
    stderr.join().map_err(|_| "Viewer error relay failed")??;
    log.lock()
        .map_err(|_| "Launch log lock failed")?
        .sync_all()?;
    if !status.success() {
        return Err(format!(
            "Viewer exited with {status}. Startup log: {}",
            log_path.display()
        )
        .into());
    }
    if options.smoke_test {
        println!(
            "Startup test passed. Capture: {}",
            folder.join("capture.png").display()
        );
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Options::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            if io::stdin().is_terminal() {
                eprintln!("Press Enter to close this error window.");
                let _ = io::stdin().read_line(&mut String::new());
            }
            ExitCode::FAILURE
        }
    }
}
