//! Campaign-sealed observation of exact authored actor/race/class inputs.
use crate::{World, foreign::Content, identity::CampaignId};
use fallout_data::{actors::initialization_inputs::Manifest, store::SourceReceipt};
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_sources: usize,
    pub max_links: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_links: 4096,
            max_projection_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("actor initialization input source cohort or campaign changed")]
    ContextChanged,
    #[error("actor initialization input canonical source identity differs")]
    SourceChanged,
    #[error("actor initialization input {0} budget exceeded")]
    Capacity(&'static str),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
    #[error(transparent)]
    Projection(#[from] serde_json::Error),
}
pub struct Requests<'a> {
    manifest: Manifest<'a>,
    campaign: CampaignId,
    cohort: String,
}
#[derive(Debug, Serialize)]
pub struct Observation<'a, 'source> {
    pub manifest: &'a Manifest<'source>,
    pub campaign: CampaignId,
    pub source_cohort_sha256: &'a str,
    pub state_revision: u64,
    pub current_actor_values: Option<serde_json::Value>,
    pub actor_reference_bound: bool,
    pub initialization_supported: bool,
    pub scope: &'static str,
}
fn same_sources(a: &[SourceReceipt], b: &[SourceReceipt]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
}
fn validate(
    world: &World<'_>,
    content: &Content,
    manifest: &Manifest<'_>,
    limits: Limits,
) -> Result<(), Error> {
    if manifest.sources().len() > limits.max_sources {
        return Err(Error::Capacity("source"));
    }
    if manifest.links().len() > limits.max_links {
        return Err(Error::Capacity("link"));
    }
    content.validate_world(world)?;
    if !same_sources(manifest.sources(), &world.catalogue().sources)
        || manifest.winning_content_sha256() != world.catalogue().winning_content_sha256()
    {
        return Err(Error::ContextChanged);
    }
    let actor = manifest.actor();
    let canonical = content.source_form(world, actor.key)?;
    if canonical.kind != actor.kind || canonical.flags != actor.source.record_flags {
        return Err(Error::SourceChanged);
    }
    for link in manifest
        .links()
        .iter()
        .filter(|l| l.direct_binding_available)
    {
        let binding = &link.association.binding;
        let key = binding.key.as_ref().ok_or(Error::SourceChanged)?;
        let target = binding.target.as_ref().ok_or(Error::SourceChanged)?;
        let canonical = content.source_form(world, key)?;
        if canonical.kind != target.kind || canonical.flags != target.record_flags {
            return Err(Error::SourceChanged);
        }
    }
    Ok(())
}
struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "actor initialization observation projection budget",
            ));
        }
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> Requests<'a> {
    pub fn prepare(
        world: &World<'_>,
        content: &Content,
        manifest: Manifest<'a>,
        limits: Limits,
    ) -> Result<Self, Error> {
        validate(world, content, &manifest, limits)?;
        let result = Self {
            manifest,
            campaign: world.campaign(),
            cohort: world.catalogue_fingerprint().into(),
        };
        result.observe(world, content, limits)?;
        Ok(result)
    }
    pub fn observe<'b>(
        &'b self,
        world: &World<'_>,
        content: &Content,
        limits: Limits,
    ) -> Result<Observation<'b, 'a>, Error> {
        if self.campaign != world.campaign() || self.cohort != world.catalogue_fingerprint() {
            return Err(Error::ContextChanged);
        }
        validate(world, content, &self.manifest, limits)?;
        let observation = Observation {
            manifest: &self.manifest,
            campaign: self.campaign,
            source_cohort_sha256: &self.cohort,
            state_revision: world.revision(),
            current_actor_values: None,
            actor_reference_bound: false,
            initialization_supported: false,
            scope: "Campaign/source-sealed exact authored initialization inputs; no bound actor reference, current/effective values, defaults, race/class application or mutation",
        };
        serde_json::to_writer(
            &mut Admission {
                bytes: 0,
                maximum: limits.max_projection_bytes,
            },
            &observation,
        )?;
        Ok(observation)
    }
}
