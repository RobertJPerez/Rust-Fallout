//! Caller-authored engineering edits through the existing canonical owner.
use super::*;
use fallout_runtime::{
    identity::{CampaignId, ReferenceId},
    reference_state::{Pose, State},
};
use serde::{Deserialize, Deserializer, de::Error as _};
use std::io::{Read, Write};

pub const REQUEST_BYTES: usize = 4096;
pub const RECEIPT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema_version: u32,
    pub scene_epoch: u64,
    pub intent_sequence: u64,
    pub expected_campaign: CampaignId,
    pub expected_catalogue_sha256: String,
    pub expected_revision: u64,
    pub key: FormKey,
    pub reference: ReferenceId,
    #[serde(deserialize_with = "complete_state")]
    pub state: State,
}

// Runtime's persisted DTO deliberately permits an absent optional scale field.
// This private request requires the caller to explicitly supply null or bits.
fn complete_state<'de, D: Deserializer<'de>>(deserializer: D) -> Result<State, D::Error> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct SourcePose {
        position_bits: [u32; 3],
        rotation_bits: [u32; 3],
        #[serde(deserialize_with = "present_scale")]
        scale_bits: Option<u32>,
    }
    fn present_scale<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u32>, D::Error> {
        Option::<u32>::deserialize(d)
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct SourceState {
        schema_version: u32,
        cell: FormKey,
        pose: SourcePose,
        enabled: bool,
    }
    let source = SourceState::deserialize(deserializer)?;
    if source.schema_version != fallout_runtime::reference_state::COMPONENT_VERSION {
        return Err(D::Error::custom("unsupported edit component schema"));
    }
    let pose = Pose::from_source(
        &fallout_data::world::Transform {
            position: source.pose.position_bits.map(f32::from_bits),
            rotation: source.pose.rotation_bits.map(f32::from_bits),
        },
        source.pose.scale_bits.map(f32::from_bits),
    )
    .map_err(D::Error::custom)?;
    State::new(source.cell, pose, source.enabled).map_err(D::Error::custom)
}

pub fn read_request(path: &Path, maximum_bytes: usize) -> model::Result<Request> {
    if maximum_bytes > REQUEST_BYTES {
        return Err("Reference edit input allowance exceeds 4 KiB".into());
    }
    let mut bytes = Vec::new();
    fallout_data::baseline::open_source(path)?
        .take(maximum_bytes as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum_bytes {
        return Err("Reference edit input byte bound exhausted".into());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    validate_header(&request)?;
    Ok(request)
}

fn validate_header(request: &Request) -> model::Result<()> {
    if request.schema_version != 1
        || request.scene_epoch == 0
        || request.intent_sequence == 0
        || request.expected_catalogue_sha256.len() != 64
        || !request
            .expected_catalogue_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid explicit reference edit schema/epoch/sequence/cohort".into());
    }
    Ok(())
}

pub(super) fn validate_display(request: &Request, displayed: &View) -> model::Result<()> {
    validate_header(request)?;
    if displayed.campaign() != request.expected_campaign
        || displayed.catalogue_fingerprint() != request.expected_catalogue_sha256
        || displayed.revision() != request.expected_revision
        || displayed.authored() != Some(&request.key)
        || displayed.reference() != request.reference
        || displayed.state().is_none()
    {
        return Err("Reference edit does not bind the current complete source view".into());
    }
    Ok(())
}

#[derive(Clone)]
pub struct Command {
    pub(super) request: Request,
    pub(super) displayed: View,
}

#[derive(Debug, Serialize)]
pub struct Receipt {
    pub schema_version: u32,
    pub scene_epoch: u64,
    pub intent_sequence: u64,
    pub before: View,
    pub after: View,
    pub canonical_commit: fallout_runtime::reference_state::Receipt,
    pub durable_publication: bool,
    pub original_gameplay_accepted: bool,
}
impl Clone for Receipt {
    fn clone(&self) -> Self {
        let commit = &self.canonical_commit;
        Self {
            schema_version: self.schema_version,
            scene_epoch: self.scene_epoch,
            intent_sequence: self.intent_sequence,
            before: self.before.clone(),
            after: self.after.clone(),
            canonical_commit: fallout_runtime::reference_state::Receipt {
                campaign: commit.campaign,
                catalogue_sha256: commit.catalogue_sha256.clone(),
                reference: commit.reference,
                authored: commit.authored.clone(),
                before_revision: commit.before_revision,
                after_revision: commit.after_revision,
                state: commit.state.clone(),
            },
            durable_publication: self.durable_publication,
            original_gameplay_accepted: self.original_gameplay_accepted,
        }
    }
}
pub struct Applied {
    pub receipt: Receipt,
    pub observation: Observation,
}

struct Bounded(Vec<u8>, usize);
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.1.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other(
                "Reference edit receipt byte bound exhausted",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn receipt_bytes(receipt: &Receipt, maximum_bytes: usize) -> model::Result<Vec<u8>> {
    if maximum_bytes > RECEIPT_BYTES {
        return Err("Reference edit receipt allowance exceeds 1 MiB".into());
    }
    let mut output = Bounded(Vec::new(), maximum_bytes.saturating_sub(1));
    serde_json::to_writer_pretty(&mut output, receipt)?;
    Ok(output.0)
}

pub(super) fn apply(
    world: &mut World<'_>,
    cell: &FormKey,
    keys: &[FormKey],
    origin: [f64; 3],
    load: LoadReceipt,
    command: Command,
) -> model::Result<Applied> {
    let Command { request, displayed } = command;
    validate_display(&request, &displayed)?;
    if keys.binary_search(&request.key).is_err()
        || world.campaign() != request.expected_campaign
        || world.catalogue_fingerprint() != request.expected_catalogue_sha256
        || world.revision() != request.expected_revision
    {
        return Err("Reference edit canonical/source boundary changed".into());
    }
    let fresh = world.reference_view(request.reference)?;
    validate_display(&request, &fresh)?;
    if fresh.state() != displayed.state()
        || fresh.state().is_none_or(|state| state.cell() != cell)
        || request.state.cell() != cell
    {
        return Err("Reference edit cannot change CELL or initialize an unavailable state".into());
    }
    // Even a byte-identical Continue creates a new runtime authority epoch.
    // Validate the actual retained display View before obtaining its fresh stage.
    drop(world.stage_reference_state(&displayed, request.state.clone())?);
    // All potentially fallible rendering work runs before canonical effects.
    // No other writer can alter this World between preflight and commit.
    let mut observation = observe(world, cell, keys, origin, load)?;
    let staged = world.stage_reference_state(&fresh, request.state.clone())?;
    let source_affine = request
        .state
        .pose()
        .source_scale()
        .map(|scale| Affine::nv_reference(&request.state.pose().source_transform(), scale))
        .transpose()?;
    let draw = if request.state.enabled() {
        let source = source_affine.ok_or("Enabled reference edit requires an explicit scale")?;
        let matrix = model::affine(source.relative_view(origin).rows);
        let transform = Transform::from_matrix(matrix);
        if !matrix.is_finite() || !transform.is_finite() {
            return Err("Reference edit overflows finite renderer placement".into());
        }
        Some(transform)
    } else {
        None
    };
    let next_revision = world
        .revision()
        .checked_add(1)
        .ok_or("Reference edit revision exhausted")?;
    let mut predicted_after = serde_json::to_value(&fresh)?;
    predicted_after["revision"] = next_revision.into();
    predicted_after["state"] = serde_json::to_value(&request.state)?;
    let predicted_commit = fallout_runtime::reference_state::Receipt {
        campaign: fresh.campaign(),
        catalogue_sha256: fresh.catalogue_fingerprint().into(),
        reference: fresh.reference(),
        authored: fresh.authored().cloned(),
        before_revision: fresh.revision(),
        after_revision: next_revision,
        state: request.state.clone(),
    };
    let predicted = serde_json::json!({
        "schema_version":1,"scene_epoch":request.scene_epoch,"intent_sequence":request.intent_sequence,
        "before":fresh,"after":predicted_after,"canonical_commit":predicted_commit,
        "durable_publication":false,"original_gameplay_accepted":false
    });
    let mut preflight = Bounded(Vec::new(), RECEIPT_BYTES - 1);
    serde_json::to_writer_pretty(&mut preflight, &predicted)?;
    let canonical_commit = world.commit_reference_state(staged)?;
    // Staging/commit changes only one component and revision, never registry
    // identity. Every key/ID below was observed successfully before that commit.
    for binding in &mut observation.report.bindings {
        if let Some(previous) = &binding.canonical {
            binding.canonical = Some(
                world
                    .reference_view(previous.reference())
                    .expect("commit preserves preflighted reference identities"),
            );
        }
        if binding.key == request.key {
            binding.source_affine = if draw.is_some() { source_affine } else { None };
            binding.display = if request.state.enabled() {
                "canonical-enabled"
            } else {
                "canonical-disabled"
            };
        }
    }
    observation.draws.insert(request.key, draw);
    observation.report.revision = next_revision;
    let after = world
        .reference_view(request.reference)
        .expect("commit preserves edited reference identity");
    Ok(Applied {
        receipt: Receipt {
            schema_version: 1,
            scene_epoch: request.scene_epoch,
            intent_sequence: request.intent_sequence,
            before: fresh,
            after,
            canonical_commit,
            durable_publication: false,
            original_gameplay_accepted: false,
        },
        observation,
    })
}
