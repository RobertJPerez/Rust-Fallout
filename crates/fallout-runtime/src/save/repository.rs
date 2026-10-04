use super::{Captured, Error, Result, format, io};
use crate::{Limits, SourceCatalogue, World, identity::CampaignId};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const MARKER: &[u8] = b"FRNATIVE1\n";
const LOCK: &[u8] = b"FRLOCK01";
static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    Current,
    Previous,
}

impl Slot {
    fn name(self) -> &'static str {
        match self {
            Self::Current => "current.frsv",
            Self::Previous => "previous.frsv",
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub enum Recovery {
    Strict,
    PreviousIfCurrentInvalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    CurrentTempWritten,
    CurrentTempSynced,
    PreviousTempSynced,
    PreviousPublished,
    CurrentPublished,
}

#[derive(Debug, Serialize)]
pub struct WriteReceipt {
    pub metadata: format::Metadata,
    pub previous_generation: Option<u64>,
    pub persistence_scope: &'static str,
}
#[derive(Debug, Serialize)]
pub struct LoadReceipt {
    pub slot: Slot,
    pub metadata: format::Metadata,
    pub current_failure: Option<String>,
    pub current_repaired: bool,
}

#[derive(Debug, Clone)]
pub struct Repository {
    root: PathBuf,
    campaign: CampaignId,
}

fn invalid(reason: &str) -> Error {
    Error::Format(reason.into())
}
fn plain(path: &Path, directory: bool) -> Result<()> {
    let metadata = fs::symlink_metadata(path).map_err(|e| io(path, e))?;
    let mut linked = metadata.file_type().is_symlink();
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        linked |= metadata.file_attributes() & 0x400 != 0;
    }
    if linked || (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(invalid(
            "save path must be a plain file/directory without a reparse point",
        ));
    }
    Ok(())
}
fn outside(root: &Path, protected: &[PathBuf]) -> Result<()> {
    for input in protected {
        let input = input.canonicalize().map_err(|e| io(input, e))?;
        if root.starts_with(&input) {
            return Err(invalid(
                "native saves must be outside protected input directories",
            ));
        }
    }
    Ok(())
}
fn read_plain(path: &Path, maximum: usize) -> Result<Vec<u8>> {
    plain(path, false)?;
    let file = File::open(path).map_err(|e| io(path, e))?;
    let length = file.metadata().map_err(|e| io(path, e))?.len();
    if length > maximum as u64 {
        return Err(invalid("save file exceeds byte budget"));
    }
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(length.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|e| io(path, e))?;
    if bytes.len() as u64 != length {
        return Err(invalid("save file changed during read"));
    }
    Ok(bytes)
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| io(path, e))?;
    file.write_all(bytes).map_err(|e| io(path, e))?;
    file.sync_all().map_err(|e| io(path, e))?;
    Ok(())
}

struct PendingFile {
    path: PathBuf,
    published: bool,
}
impl PendingFile {
    fn prepare(root: &Path, bytes: &[u8], mut observe: impl FnMut(bool)) -> Result<Self> {
        let counter = NEXT_TEMP
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| invalid("temporary name counter exhausted"))?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| invalid("system clock predates native save epoch"))?
            .as_nanos();
        let path = root.join(format!(".pending-{}-{nanos}-{counter}", std::process::id()));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|e| io(&path, e))?;
        let pending = Self {
            path,
            published: false,
        };
        // The guard owns only this newly created file. It cannot remove an
        // existing destination when temporary creation fails.
        file.write_all(bytes).map_err(|e| io(&pending.path, e))?;
        observe(false);
        file.sync_all().map_err(|e| io(&pending.path, e))?;
        observe(true);
        drop(file);
        Ok(pending)
    }
    fn publish(&mut self, destination: &Path) -> Result<()> {
        match fs::symlink_metadata(destination) {
            Ok(_) => plain(destination, false)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(io(destination, e)),
        }
        fs::rename(&self.path, destination).map_err(|e| io(destination, e))?;
        self.published = true;
        Ok(())
    }
}
impl Drop for PendingFile {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Repository {
    /// Creation requires a new directory. Opening an existing repository needs
    /// its native marker; ordinary folders and original save locations are not
    /// adopted or overwritten. Parents must already exist.
    pub fn create(path: &Path, protected: &[PathBuf], campaign: CampaignId) -> Result<Self> {
        CampaignId::from_bytes(campaign.bytes())?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let parent = parent.canonicalize().map_err(|e| io(parent, e))?;
        plain(&parent, true)?;
        let name = path
            .file_name()
            .ok_or_else(|| invalid("save repository needs a directory name"))?;
        let root = parent.join(name);
        outside(&root, protected)?;
        fs::create_dir(&root).map_err(|e| io(&root, e))?;
        write_new(
            &root.join(".rust-fallout-saves"),
            &[MARKER, &campaign.bytes()].concat(),
        )?;
        write_new(&root.join("writer.lock"), LOCK)?;
        let repository = Self { root, campaign };
        repository.sync_directory()?;
        Ok(repository)
    }
    pub fn open(path: &Path, protected: &[PathBuf]) -> Result<Self> {
        plain(path, true)?;
        let root = path.canonicalize().map_err(|e| io(path, e))?;
        outside(&root, protected)?;
        let marker = read_plain(&root.join(".rust-fallout-saves"), MARKER.len() + 16)?;
        if marker.len() != MARKER.len() + 16 || !marker.starts_with(MARKER) {
            return Err(invalid("unrecognized native save repository marker"));
        }
        let campaign = CampaignId::from_bytes(
            marker[MARKER.len()..]
                .try_into()
                .expect("checked marker extent"),
        )?;
        let repository = Self { root, campaign };
        repository.validate_marker()?;
        plain(&repository.root.join("writer.lock"), false)?;
        Ok(repository)
    }
    pub fn path(&self) -> &Path {
        &self.root
    }
    fn validate_marker(&self) -> Result<()> {
        plain(&self.root, true)?;
        if read_plain(&self.root.join(".rust-fallout-saves"), MARKER.len() + 16)?
            != [MARKER, &self.campaign.bytes()].concat()
        {
            return Err(invalid("unrecognized native save repository marker"));
        }
        Ok(())
    }
    fn lock(&self) -> Result<File> {
        self.validate_marker()?;
        let path = self.root.join("writer.lock");
        plain(&path, false)?;
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| io(&path, e))?;
        match file.try_lock() {
            Ok(()) => (),
            Err(TryLockError::WouldBlock) => return Err(Error::Busy),
            Err(TryLockError::Error(e)) => return Err(io(&path, e)),
        }
        let mut marker = Vec::new();
        (&mut file)
            .take((LOCK.len() + 1) as u64)
            .read_to_end(&mut marker)
            .map_err(|e| io(&path, e))?;
        if marker != LOCK {
            return Err(invalid("native writer-lock identity mismatch"));
        }
        Ok(file)
    }
    fn sync_directory(&self) -> Result<()> {
        #[cfg(unix)]
        {
            File::open(&self.root)
                .and_then(|file| file.sync_all())
                .map_err(|e| io(&self.root, e))?;
        }
        // Rust's safe std API has no equivalent directory flush here on Windows.
        // Same-directory replacement and process-interruption recovery are
        // tested separately; receipts do not claim power-loss durability.
        Ok(())
    }
    fn slot_bytes(&self, slot: Slot, limits: Limits) -> Result<Vec<u8>> {
        let maximum = limits
            .max_snapshot_bytes
            .checked_add(format::OVERHEAD)
            .ok_or_else(|| invalid("save file budget overflow"))?;
        read_plain(&self.root.join(slot.name()), maximum)
    }
    pub fn commit(&self, capture: &Captured) -> Result<WriteReceipt> {
        self.commit_observing(capture, |_| {})
    }
    /// Stage observations support progress reporting and deterministic process
    /// interruption tests. They are notifications, not a publication bypass.
    pub fn commit_observing(
        &self,
        capture: &Captured,
        mut observe: impl FnMut(Stage),
    ) -> Result<WriteReceipt> {
        let _lock = self.lock()?;
        if capture.snapshot.campaign != self.campaign {
            return Err(invalid(
                "captured campaign differs from repository identity",
            ));
        }
        capture
            .source_validation
            .check(&capture.snapshot, capture.limits)?;
        let current = self.root.join(Slot::Current.name());
        let previous = if current.try_exists().map_err(|e| io(&current, e))? {
            let bytes = self.slot_bytes(Slot::Current, capture.limits)?;
            let decoded = format::decode(&bytes, capture.limits)?;
            if decoded.snapshot.campaign != capture.snapshot.campaign
                || decoded.snapshot.catalogue_sha256 != capture.snapshot.catalogue_sha256
            {
                return Err(invalid(
                    "campaign/content cohort changed; use a separate native repository",
                ));
            }
            capture
                .source_validation
                .check(&decoded.snapshot, capture.limits)?;
            if decoded.snapshot.state_revision > capture.snapshot.state_revision
                || (decoded.snapshot.state_revision == capture.snapshot.state_revision
                    && decoded.snapshot != capture.snapshot)
                || !decoded
                    .snapshot
                    .clocks
                    .no_later_than(capture.snapshot.clocks)
                || decoded.snapshot.next_item > capture.snapshot.next_item
                || decoded.snapshot.next_instance > capture.snapshot.next_instance
                || decoded.snapshot.next_reference > capture.snapshot.next_reference
                || decoded.snapshot.next_event_sequence > capture.snapshot.next_event_sequence
            {
                return Err(invalid(
                    "captured request would replace newer canonical state",
                ));
            }
            Some((bytes, decoded.metadata.generation))
        } else {
            let backup = self.root.join(Slot::Previous.name());
            if backup.try_exists().map_err(|e| io(&backup, e))? {
                return Err(invalid(
                    "previous slot exists without current; recover it explicitly",
                ));
            }
            None
        };
        let generation = previous
            .as_ref()
            .map_or(Some(1), |(_, generation)| generation.checked_add(1))
            .ok_or_else(|| invalid("save generation exhausted"))?;
        let bytes = format::encode(capture, generation)?;
        let metadata = format::decode(&bytes, capture.limits)?.metadata;
        let mut pending = PendingFile::prepare(&self.root, &bytes, |synced| {
            observe(if synced {
                Stage::CurrentTempSynced
            } else {
                Stage::CurrentTempWritten
            })
        })?;
        if let Some((old, _)) = &previous {
            let mut backup = PendingFile::prepare(&self.root, old, |synced| {
                if synced {
                    observe(Stage::PreviousTempSynced);
                }
            })?;
            backup.publish(&self.root.join(Slot::Previous.name()))?;
            self.sync_directory()?;
            observe(Stage::PreviousPublished);
        }
        pending.publish(&current)?;
        self.sync_directory()?;
        observe(Stage::CurrentPublished);
        Ok(WriteReceipt {
            metadata,
            previous_generation: previous.map(|(_, generation)| generation),
            persistence_scope: "Synced temporary files and same-directory replacement; process interruption tested separately; Windows directory/power-loss durability is unverified",
        })
    }
    fn load_slot<'a>(
        &self,
        slot: Slot,
        catalogue: SourceCatalogue<'a>,
        limits: Limits,
    ) -> Result<(World<'a>, format::Metadata)> {
        let decoded = format::decode(&self.slot_bytes(slot, limits)?, limits)?;
        if decoded.metadata.campaign != self.campaign {
            return Err(invalid("save campaign differs from repository identity"));
        }
        let world = World::restore(catalogue, decoded.snapshot, limits)?;
        Ok((world, decoded.metadata))
    }
    pub fn load<'a>(
        &self,
        catalogue: impl Into<SourceCatalogue<'a>>,
        limits: Limits,
        recovery: Recovery,
    ) -> Result<(World<'a>, LoadReceipt)> {
        let _lock = self.lock()?;
        let catalogue = catalogue.into();
        match self.load_slot(Slot::Current, catalogue.clone(), limits) {
            Ok((world, metadata)) => Ok((
                world,
                LoadReceipt {
                    slot: Slot::Current,
                    metadata,
                    current_failure: None,
                    current_repaired: false,
                },
            )),
            Err(error) => match recovery {
                Recovery::Strict => Err(error),
                Recovery::PreviousIfCurrentInvalid => {
                    let (world, metadata) = self.load_slot(Slot::Previous, catalogue, limits)?;
                    Ok((
                        world,
                        LoadReceipt {
                            slot: Slot::Previous,
                            metadata,
                            current_failure: Some(error.to_string()),
                            current_repaired: false,
                        },
                    ))
                }
            },
        }
    }
    /// Repair is explicit and only copies a fully restored previous snapshot.
    /// Loading with recovery alone leaves the failed current file untouched.
    pub fn recover_previous<'a>(
        &self,
        catalogue: impl Into<SourceCatalogue<'a>>,
        limits: Limits,
    ) -> Result<(World<'a>, LoadReceipt)> {
        let _lock = self.lock()?;
        let catalogue = catalogue.into();
        let error = match self.load_slot(Slot::Current, catalogue.clone(), limits) {
            Ok(_) => {
                return Err(invalid(
                    "current slot is valid; previous-slot repair is unnecessary",
                ));
            }
            Err(error) => error.to_string(),
        };
        let bytes = self.slot_bytes(Slot::Previous, limits)?;
        let decoded = format::decode(&bytes, limits)?;
        if decoded.metadata.campaign != self.campaign {
            return Err(invalid(
                "previous save campaign differs from repository identity",
            ));
        }
        let world = World::restore(catalogue, decoded.snapshot, limits)?;
        let mut pending = PendingFile::prepare(&self.root, &bytes, |_| {})?;
        pending.publish(&self.root.join(Slot::Current.name()))?;
        self.sync_directory()?;
        Ok((
            world,
            LoadReceipt {
                slot: Slot::Previous,
                metadata: decoded.metadata,
                current_failure: Some(error),
                current_repaired: true,
            },
        ))
    }
}
#[cfg(test)]
mod publication_preflight_tests {
    use super::*;
    use crate::{
        events::Clocks,
        identity::ReferenceId,
        inventory::{Bank, Facts, Item, ItemId, Ownership},
        snapshot::{Reference, Snapshot},
    };
    use fallout_data::identity::{FormKey, ProfileId};

    fn capture(revision: u64) -> Captured {
        let owner = ReferenceId(1.try_into().unwrap());
        let mut facts = Facts::unknown(FormKey {
            profile: ProfileId::NvOriginal,
            origin_plugin: "falloutnv.esm".into(),
            local_id: 0x500,
        });
        facts.ownership = Some(Ownership::Live { reference: owner });
        Captured {
            source_validation: Default::default(),
            limits: Limits {
                max_snapshot_bytes: 4096,
                ..Default::default()
            },
            snapshot: Snapshot {
                schema_version: crate::snapshot::SCHEMA_VERSION,
                campaign: CampaignId::from_bytes([0x4c; 16]).unwrap(),
                state_revision: revision,
                profile: ProfileId::NvOriginal,
                // Publication needs intrinsic identity validation, not a source
                // catalogue. Source-bound restoration remains a separate gate.
                catalogue_sha256: "00".repeat(32),
                next_item: 2,
                next_instance: 1,
                next_reference: 2,
                next_event_sequence: 1,
                clocks: Clocks::default(),
                references: vec![Reference {
                    id: owner,
                    authored: None,
                }],
                instances: Vec::new(),
                pending_events: Vec::new(),
                inventory_banks: vec![Bank {
                    owner,
                    items: vec![Item {
                        id: ItemId(1.try_into().unwrap()),
                        owner,
                        count: 7.try_into().unwrap(),
                        facts,
                    }],
                }],
            },
        }
    }
    fn slots(repository: &Repository) -> [Vec<u8>; 2] {
        ["current.frsv", "previous.frsv"]
            .map(|name| fs::read(repository.path().join(name)).unwrap())
    }
    fn assert_no_temporary(repository: &Repository) {
        assert_eq!(fs::read_dir(repository.path()).unwrap().count(), 4);
    }

    #[test]
    fn checksummed_unrestorable_current_cannot_replace_a_valid_previous_slot() {
        let directory = tempfile::tempdir().unwrap();
        let first = capture(10);
        let repository = Repository::create(
            &directory.path().join("native"),
            &[],
            first.snapshot.campaign,
        )
        .unwrap();
        repository.commit(&first).unwrap();
        let second = capture(11);
        repository.commit(&second).unwrap();
        let original = slots(&repository);
        let mut bad = second.clone();
        bad.snapshot.inventory_banks[0].items[0].facts.ownership = Some(Ownership::Live {
            reference: ReferenceId(99.try_into().unwrap()),
        });
        let bytes = format::encode(&bad, 2).unwrap();
        // Framing/typed DTO validation succeeds; the persistent link is invalid.
        format::decode(&bytes, bad.limits).unwrap();
        fs::write(repository.path().join("current.frsv"), &bytes).unwrap();
        assert!(matches!(
            repository.commit(&capture(12)),
            Err(Error::State(crate::Error::MissingReference))
        ));
        assert_eq!(slots(&repository), [bytes, original[1].clone()]);
        assert_no_temporary(&repository);
        fs::write(repository.path().join("current.frsv"), &original[0]).unwrap();
        assert_eq!(
            repository.commit(&capture(12)).unwrap().metadata.generation,
            3
        );
        assert_eq!(slots(&repository)[1], original[0]);
    }

    #[test]
    fn invalid_proposed_links_or_allocators_leave_both_slots_unchanged_and_allow_retry() {
        let directory = tempfile::tempdir().unwrap();
        let first = capture(10);
        let repository = Repository::create(
            &directory.path().join("native"),
            &[],
            first.snapshot.campaign,
        )
        .unwrap();
        repository.commit(&first).unwrap();
        repository.commit(&capture(11)).unwrap();
        let original = slots(&repository);
        let mut bad = capture(12);
        bad.snapshot.inventory_banks[0].items[0]
            .facts
            .script_instance = Some(crate::identity::InstanceId(99.try_into().unwrap()));
        assert!(matches!(
            repository.commit(&bad),
            Err(Error::State(crate::Error::MissingInstance))
        ));
        assert_eq!(slots(&repository), original);
        assert_no_temporary(&repository);
        let mut bad = capture(12);
        bad.snapshot.next_item = 0;
        assert_eq!(
            repository.commit(&bad).unwrap_err().to_string(),
            "runtime state is invalid: snapshot allocator cannot be zero"
        );
        assert_eq!(slots(&repository), original);
        assert_no_temporary(&repository);
        let mut bad = capture(12);
        bad.snapshot
            .references
            .push(bad.snapshot.references[0].clone());
        assert_eq!(
            repository.commit(&bad).unwrap_err().to_string(),
            "runtime state is invalid: duplicate reference or allocator would reuse an identity"
        );
        assert_eq!(slots(&repository), original);
        assert_no_temporary(&repository);
        assert_eq!(
            repository.commit(&capture(12)).unwrap().metadata.generation,
            3
        );
        assert_eq!(slots(&repository)[1], original[0]);
    }
}
