use super::*;
use crate::{
    identity::ProfileId,
    vfs::{AssetPath, AssetSource},
};
use sha2::Digest;
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    sync::{Condvar, OnceLock},
    time::Instant,
};

pub(crate) struct Pause {
    after_extract: bool,
    state: Mutex<(bool, bool, bool)>,
    changed: Condvar,
}
impl Pause {
    pub(crate) fn new(after_extract: bool) -> Arc<Self> {
        Arc::new(Self {
            after_extract,
            state: Mutex::new((false, false, false)),
            changed: Condvar::new(),
        })
    }
    pub(super) fn arrive(&self, after_extract: bool) {
        if after_extract != self.after_extract {
            return;
        }
        let mut state = self.state.lock().unwrap();
        state.0 = true;
        self.changed.notify_all();
        while !state.1 {
            state = self.changed.wait(state).unwrap();
        }
    }
    pub(crate) fn reached(&self) {
        let state = self.state.lock().unwrap();
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(10), |state| !state.0)
            .unwrap();
        assert!(
            state.0 && !timeout.timed_out(),
            "worker did not reach controlled boundary"
        );
    }
    pub(crate) fn release(&self) {
        self.state.lock().unwrap().1 = true;
        self.changed.notify_all();
    }
    pub(super) fn sent(&self) {
        self.state.lock().unwrap().2 = true;
        self.changed.notify_all();
    }
    pub(crate) fn completed(&self) {
        let state = self.state.lock().unwrap();
        let (state, timeout) = self
            .changed
            .wait_timeout_while(state, Duration::from_secs(10), |state| !state.2)
            .unwrap();
        assert!(
            state.2 && !timeout.timed_out(),
            "worker did not send completion"
        );
    }
}
// Release test workers on assertion unwinding before a pool's Drop joins them.
struct Release(Arc<Pause>);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}
type CacheHooks = Mutex<BTreeMap<PathBuf, Arc<Pause>>>;
static CACHE_HOOKS: OnceLock<CacheHooks> = OnceLock::new();
pub(crate) fn pause_cache(root: &Path, marker: bool) {
    if !marker {
        return;
    }
    let pause = CACHE_HOOKS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .get(root)
        .cloned();
    if let Some(pause) = pause {
        pause.arrive(true);
    }
}
struct CacheHook(PathBuf, Arc<Pause>);
impl Drop for CacheHook {
    fn drop(&mut self) {
        self.1.release();
        CACHE_HOOKS.get().unwrap().lock().unwrap().remove(&self.0);
    }
}

pub(crate) struct Fixture {
    pub(crate) source: tempfile::TempDir,
    pub(crate) cache: tempfile::TempDir,
    path: PathBuf,
    pub(crate) member_path: AssetPath,
    pub(crate) source_member: AssetSource,
}
impl Fixture {
    pub(crate) fn new(payload: &[u8], compressed: bool) -> Self {
        // Independently authored one-folder/one-file BSA104. No upstream assets.
        // Header36 + folder16 + bzstring8 + file16 + filename6 = payload offset82.
        let stored = if compressed {
            use std::io::Write;
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(payload).unwrap();
            let mut stored = (payload.len() as u32).to_le_bytes().to_vec();
            stored.extend(encoder.finish().unwrap());
            stored
        } else {
            payload.to_vec()
        };
        let mut bytes = vec![0u8; 82];
        bytes[..4].copy_from_slice(b"BSA\0");
        for (at, word) in [
            (4, 104),
            (8, 36),
            (12, if compressed { 7 } else { 3 }),
            (16, 1),
            (20, 1),
            (24, 7),
            (28, 6),
            (44, 1),
            (48, 52),
            (68, stored.len() as u32),
            (72, 82),
        ] {
            bytes[at..at + 4].copy_from_slice(&word.to_le_bytes());
        }
        bytes[52] = 7;
        bytes[53..60].copy_from_slice(b"meshes\0");
        bytes[76..82].copy_from_slice(b"a.nif\0");
        bytes.extend(stored);
        let source = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let path = source.path().join("authored.bsa");
        fs::write(&path, bytes).unwrap();
        Self {
            source,
            cache,
            source_member: AssetSource {
                container: path.display().to_string(),
                entry_index: 0,
                original_path: b"meshes\\a.nif".to_vec(),
            },
            path,
            member_path: AssetPath::new(b"meshes/a.nif").unwrap(),
        }
    }
    fn input(&self) -> Arc<ArchiveInput> {
        ArchiveInput::open(&self.path).unwrap()
    }
    fn member(&self, input: &Arc<ArchiveInput>) -> Member {
        input
            .member(&self.member_path, &self.source_member)
            .unwrap()
    }
    fn destination(&self) -> Option<(PathBuf, PathBuf)> {
        Some((
            self.cache.path().to_path_buf(),
            self.source.path().to_path_buf(),
        ))
    }
    fn pool(&self, limits: Limits) -> (Generation, ResourceJobs) {
        let generation = Generation::new("a".repeat(64)).unwrap();
        let jobs = ResourceJobs::new(limits, generation.clone()).unwrap();
        (generation, jobs)
    }
}

fn eventually(mut condition: impl FnMut() -> bool) {
    let start = Instant::now();
    while !condition() {
        assert!(start.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn cold_retry_and_corrupt_or_partial_reuse() {
    let fixture = Fixture::new(b"independently authored payload", true);
    let original = fs::read(&fixture.path).unwrap();
    let input = fixture.input();
    let identity = fixture.member(&input).identity.clone();
    let (generation, jobs) = fixture.pool(Limits::default());
    let mut first = jobs
        .submit(
            fixture.member(&input),
            generation.token().unwrap(),
            fixture.destination(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(first.bytes(), b"independently authored payload");
    assert!(!first.take_cache_receipt().unwrap().reused);
    drop(first);
    let mut second = jobs
        .submit(
            fixture.member(&input),
            generation.token().unwrap(),
            fixture.destination(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert!(second.take_cache_receipt().unwrap().reused);
    drop(second);
    let key = identity.key().unwrap();
    fs::write(fixture.cache.path().join(format!("{key}.blob")), b"partial").unwrap();
    let error = jobs
        .submit(
            fixture.member(&input),
            generation.token().unwrap(),
            fixture.destination(),
        )
        .unwrap()
        .wait();
    assert!(
        matches!(error,Err(JobError::Failed(crate::Error::Resolution(reason))) if reason.contains("length mismatch"))
    );
    assert_eq!(jobs.usage(), Usage::default());
    assert_eq!(fs::read(&fixture.path).unwrap(), original);
}

#[test]
fn obsolete_extraction_cannot_publish_and_new_generation_can_retry() {
    let fixture = Fixture::new(b"payload", true);
    let input = fixture.input();
    let identity = fixture.member(&input).identity.clone();
    let (generation, jobs) = fixture.pool(Limits::default());
    let pause = Pause::new(true);
    let handle = jobs
        .submit_inner(
            fixture.member(&input),
            generation.token().unwrap(),
            fixture.destination(),
            Some(pause.clone()),
        )
        .unwrap();
    let release = Release(pause.clone());
    pause.reached();
    generation.advance("b".repeat(64)).unwrap();
    pause.release();
    assert!(matches!(handle.wait(), Err(JobError::Stale)));
    drop(handle);
    eventually(|| jobs.usage() == Usage::default());
    assert!(
        cache::read_verified(fixture.cache.path(), fixture.source.path(), &identity, 64)
            .unwrap()
            .is_none()
    );
    let result = jobs
        .submit(
            fixture.member(&input),
            generation.token().unwrap(),
            fixture.destination(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(result.bytes(), b"payload");
    drop(release);
}

#[test]
fn cancelled_staged_marker_leaves_verified_orphan_for_retry() {
    let fixture = Fixture::new(b"payload", false);
    let input = fixture.input();
    let identity = fixture.member(&input).identity.clone();
    let (generation, jobs) = fixture.pool(Limits::default());
    let pause = Pause::new(true);
    let root = fixture.cache.path().canonicalize().unwrap();
    CACHE_HOOKS
        .get_or_init(Default::default)
        .lock()
        .unwrap()
        .insert(root.clone(), pause.clone());
    let hook = CacheHook(root, pause.clone());
    let handle = jobs
        .submit(
            fixture.member(&input),
            generation.token().unwrap(),
            fixture.destination(),
        )
        .unwrap();
    pause.reached();
    handle.cancel();
    pause.release();
    assert!(matches!(handle.wait(), Err(JobError::Cancelled)));
    drop(handle);
    eventually(|| jobs.usage() == Usage::default());
    drop(hook);
    let key = identity.key().unwrap();
    assert!(fixture.cache.path().join(format!("{key}.blob")).exists());
    assert!(
        cache::read_verified(fixture.cache.path(), fixture.source.path(), &identity, 64)
            .unwrap()
            .is_none()
    );
    // RAII rolls back the unpublished temporary marker.
    assert_eq!(fs::read_dir(fixture.cache.path()).unwrap().count(), 1);
    let artifact = jobs
        .submit(
            fixture.member(&input),
            generation.token().unwrap(),
            fixture.destination(),
        )
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(artifact.bytes(), b"payload");
}

#[test]
fn completed_output_remains_bounded_and_stale_admission_releases_it() {
    let fixture = Fixture::new(b"payload", false);
    let input = fixture.input();
    let (generation, jobs) = fixture.pool(Limits {
        workers: 1,
        outstanding: 1,
        decoded_bytes: 7,
    });
    let pause = Pause::new(true);
    let handle = jobs
        .submit_inner(
            fixture.member(&input),
            generation.token().unwrap(),
            None,
            Some(pause.clone()),
        )
        .unwrap();
    let release = Release(pause.clone());
    pause.reached();
    pause.release();
    pause.completed(); // completion is queued; the public consumer has not read it
    assert_eq!(
        jobs.usage(),
        Usage {
            outstanding: 1,
            decoded_bytes: 7
        }
    );
    assert!(matches!(
        jobs.submit(fixture.member(&input), generation.token().unwrap(), None),
        Err(JobError::QueueFull)
    ));
    generation.advance("a".repeat(64)).unwrap();
    assert!(matches!(handle.try_take(), Err(JobError::Stale)));
    assert_eq!(jobs.usage(), Usage::default());
    let artifact = jobs
        .submit(fixture.member(&input), generation.token().unwrap(), None)
        .unwrap()
        .wait()
        .unwrap();
    assert_eq!(jobs.usage().decoded_bytes, 7);
    drop(artifact);
    assert_eq!(jobs.usage(), Usage::default());
    drop(release);
}

#[test]
fn queued_cancel_and_drop_release_source_and_budget_pins() {
    let fixture = Fixture::new(b"payload", false);
    let input = fixture.input();
    let weak = Arc::downgrade(&input);
    let (generation, jobs) = fixture.pool(Limits {
        workers: 1,
        outstanding: 2,
        decoded_bytes: 14,
    });
    let pause = Pause::new(false);
    let first = jobs
        .submit_inner(
            fixture.member(&input),
            generation.token().unwrap(),
            None,
            Some(pause.clone()),
        )
        .unwrap();
    let release = Release(pause.clone());
    pause.reached();
    let second = jobs
        .submit(fixture.member(&input), generation.token().unwrap(), None)
        .unwrap();
    drop(input);
    second.cancel();
    drop(second);
    drop(first);
    pause.release();
    eventually(|| jobs.usage() == Usage::default() && weak.upgrade().is_none());
    // A Windows protected source handle has actually been released.
    fs::write(&fixture.path, b"source release verified").unwrap();
    drop(release);
}

#[test]
fn budget_identity_and_shutdown_admission_fail_for_specific_reasons() {
    let fixture = Fixture::new(b"payload", false);
    let input = fixture.input();
    let (generation, jobs) = fixture.pool(Limits {
        workers: 1,
        outstanding: 2,
        decoded_bytes: 6,
    });
    assert!(matches!(
        jobs.submit(fixture.member(&input), generation.token().unwrap(), None),
        Err(JobError::ByteBudget)
    ));
    assert_eq!(jobs.usage(), Usage::default());
    let other = Generation::new("a".repeat(64)).unwrap();
    assert!(
        matches!(jobs.submit(fixture.member(&input),other.token().unwrap(),None),Err(JobError::Invalid(reason)) if reason.contains("another resource generation"))
    );
    let mut wrong = fixture.source_member.clone();
    wrong.entry_index = 1;
    assert!(
        matches!(input.member(&fixture.member_path,&wrong),Err(JobError::Invalid(reason)) if reason.contains("disappeared"))
    );
    let token = generation.token().unwrap();
    drop(jobs);
    assert!(matches!(token.check(), Err(JobError::Closed)));
    assert!(matches!(
        Generation::new("invalid".into()),
        Err(JobError::Invalid(_))
    ));
    let identity = fixture.member(&input).identity;
    assert_eq!(identity.profile, ProfileId::NvOriginal);
}

#[test]
fn unconsumed_failure_keeps_the_outstanding_limit_until_observed() {
    let fixture = Fixture::new(b"payload", false);
    let input = fixture.input();
    let (generation, jobs) = fixture.pool(Limits {
        workers: 1,
        outstanding: 1,
        decoded_bytes: 7,
    });
    let pause = Pause::new(false);
    let handle = jobs
        .submit_inner(
            fixture.member(&input),
            generation.token().unwrap(),
            Some((
                fixture.source.path().to_path_buf(),
                fixture.source.path().to_path_buf(),
            )),
            Some(pause.clone()),
        )
        .unwrap();
    let release = Release(pause.clone());
    pause.reached();
    pause.release();
    pause.completed();
    assert!(matches!(
        jobs.submit(fixture.member(&input), generation.token().unwrap(), None),
        Err(JobError::QueueFull)
    ));
    assert!(
        matches!(handle.wait(),Err(JobError::Failed(crate::Error::Resolution(reason))) if reason.contains("outside the installation"))
    );
    assert_eq!(jobs.usage(), Usage::default());
    drop(release);
}

#[test]
fn changed_archive_source_uses_a_new_key_without_losing_prior_cache() {
    let fixture = Fixture::new(b"first", true);
    let input = fixture.input();
    let (generation, jobs) = fixture.pool(Limits::default());
    let mut first = jobs
        .submit(
            fixture.member(&input),
            generation.token().unwrap(),
            fixture.destination(),
        )
        .unwrap()
        .wait()
        .unwrap();
    let first_receipt = first.take_cache_receipt().unwrap();
    drop(first);
    drop(input);
    let replacement = Fixture::new(b"second", true);
    fs::write(&fixture.path, fs::read(&replacement.path).unwrap()).unwrap();
    let input = fixture.input();
    let mut second = jobs
        .submit(
            fixture.member(&input),
            generation.token().unwrap(),
            fixture.destination(),
        )
        .unwrap()
        .wait()
        .unwrap();
    let second_receipt = second.take_cache_receipt().unwrap();
    assert_eq!(second.bytes(), b"second");
    assert_ne!(first_receipt.key, second_receipt.key);
    assert_ne!(
        first_receipt.manifest.identity.source_sha256,
        second_receipt.manifest.identity.source_sha256
    );
    assert!(
        cache::read_verified(
            fixture.cache.path(),
            fixture.source.path(),
            &first_receipt.manifest.identity,
            64
        )
        .unwrap()
        .is_some()
    );
}

#[test]
fn internally_consistent_partial_marker_cannot_admit_an_incomplete_member() {
    let fixture = Fixture::new(b"complete source payload", true);
    let input = fixture.input();
    let (generation, jobs) = fixture.pool(Limits::default());
    let mut artifact = jobs
        .submit(
            fixture.member(&input),
            generation.token().unwrap(),
            fixture.destination(),
        )
        .unwrap()
        .wait()
        .unwrap();
    let mut receipt = artifact.take_cache_receipt().unwrap();
    drop(artifact);
    let partial = b"partial";
    receipt.manifest.bytes = partial.len() as u64;
    receipt.manifest.sha256 = format!("{:x}", sha2::Sha256::digest(partial));
    fs::write(
        fixture.cache.path().join(format!("{}.blob", receipt.key)),
        partial,
    )
    .unwrap();
    fs::write(
        fixture.cache.path().join(format!("{}.json", receipt.key)),
        serde_json::to_vec(&receipt.manifest).unwrap(),
    )
    .unwrap();
    let handle = jobs
        .submit(
            fixture.member(&input),
            generation.token().unwrap(),
            fixture.destination(),
        )
        .unwrap();
    assert!(
        matches!(handle.wait(),Err(JobError::Invalid(reason)) if reason.contains("decoded length differs from source"))
    );
    assert_eq!(jobs.usage(), Usage::default());
}
