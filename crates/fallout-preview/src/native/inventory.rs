//! One source/display-bound read-only observation from the existing item owner.
use super::*;
use fallout_runtime::{
    identity::{CampaignId, ReferenceId},
    inventory::{InventoryView, ViewLimits},
};
use serde::Deserialize;
use std::io::{Read, Write};

pub const REQUEST_BYTES: usize = 4096;
pub const OUTPUT_BYTES: usize = 1024 * 1024;
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub max_items: usize,
    pub max_links: usize,
    pub max_extra_bytes: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub scene_epoch: u64,
    pub sequence: u64,
    pub expected_campaign: CampaignId,
    pub expected_catalogue_sha256: String,
    pub expected_revision: u64,
    pub key: FormKey,
    pub owner: ReferenceId,
    pub limits: Limits,
    pub output_bytes: usize,
}
impl Request {
    fn validate(&self) -> model::Result<()> {
        if self.schema_version != 1
            || self.scene_epoch == 0
            || self.sequence == 0
            || self.expected_catalogue_sha256.len() != 64
            || !self
                .expected_catalogue_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
            || self.limits.max_items > 128
            || self.limits.max_links > 1024
            || self.limits.max_extra_bytes > 65536
            || self.output_bytes == 0
            || self.output_bytes > OUTPUT_BYTES
        {
            return Err("Invalid inventory query identity or lower-only limits".into());
        }
        Ok(())
    }
    pub(super) fn validate_display(&self, view: &View) -> model::Result<()> {
        self.validate()?;
        if self.expected_campaign != view.campaign()
            || self.expected_catalogue_sha256 != view.catalogue_fingerprint()
            || self.expected_revision != view.revision()
            || view.authored() != Some(&self.key)
            || self.owner != view.reference()
            || view.state().is_none()
        {
            return Err("Inventory query does not bind a current complete displayed owner".into());
        }
        Ok(())
    }
}
pub fn read_request(path: &Path, maximum_bytes: usize) -> model::Result<Request> {
    if maximum_bytes > REQUEST_BYTES {
        return Err("Inventory query input allowance exceeds 4 KiB".into());
    }
    let mut bytes = Vec::new();
    fallout_data::baseline::open_source(path)?
        .take(maximum_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum_bytes {
        return Err("Inventory query input byte bound exhausted".into());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    request.validate()?;
    Ok(request)
}
#[derive(Clone)]
pub struct Command {
    pub(super) request: Request,
    pub(super) displayed: View,
}
#[derive(Serialize)]
pub struct Observation {
    pub schema_version: u32,
    pub scene_epoch: u64,
    pub sequence: u64,
    pub view: InventoryView,
    pub original_inventory_ui_accepted: bool,
    #[serde(skip)]
    pub report_bytes: Arc<[u8]>,
    #[serde(skip)]
    pub(super) accepted: bool,
}
impl Observation {
    pub fn status(&self) -> String {
        let label = match self.view.items() {
            None => "Inventory unavailable".into(),
            Some([]) => "Inventory initialized empty".into(),
            Some(items) => format!("Inventory {} retained lots", items.len()),
        };
        format!("{label}; revision {}", self.view.revision())
    }
    pub(super) fn matches(&self, scene_epoch: u64, displayed: &View) -> bool {
        self.scene_epoch == scene_epoch
            && self.view.owner() == displayed.reference()
            && self.view.authored() == displayed.authored()
            && self.view.campaign() == displayed.campaign()
            && self.view.catalogue_fingerprint() == displayed.catalogue_fingerprint()
            && self.view.revision() == displayed.revision()
    }
}
struct Bounded(Vec<u8>, usize);
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.1.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other(
                "Inventory observation output byte bound exhausted",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(super) fn observe(
    world: &World<'_>,
    keys: &[FormKey],
    command: Command,
) -> model::Result<Observation> {
    let Command { request, displayed } = command;
    request.validate_display(&displayed)?;
    if keys.binary_search(&request.key).is_err()
        || world.campaign() != request.expected_campaign
        || world.catalogue_fingerprint() != request.expected_catalogue_sha256
        || world.revision() != request.expected_revision
    {
        return Err("Inventory query canonical/source boundary changed".into());
    }
    // Existing sealed View staging checks its authority epoch; dropping the
    // stage does not mutate or initialize any reference/inventory component.
    drop(world.stage_reference_state(
        &displayed,
        displayed.state().expect("checked complete view").clone(),
    )?);
    let view = world.inventory_view(
        request.owner,
        ViewLimits {
            max_items: request.limits.max_items,
            max_links: request.limits.max_links,
            max_extra_bytes: request.limits.max_extra_bytes,
        },
    )?;
    let mut observation = Observation {
        schema_version: 1,
        scene_epoch: request.scene_epoch,
        sequence: request.sequence,
        view,
        original_inventory_ui_accepted: false,
        report_bytes: Arc::from([]),
        accepted: false,
    };
    let mut bytes = Bounded(Vec::new(), request.output_bytes - 1);
    serde_json::to_writer_pretty(&mut bytes, &observation)?;
    bytes.0.push(b'\n');
    observation.report_bytes = Arc::from(bytes.0);
    Ok(observation)
}
