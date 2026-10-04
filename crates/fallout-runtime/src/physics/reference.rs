//! Source-qualified engineering collision selections over an existing World.
//! Explicit query frames are distinct from source DATA and saved canonical pose.
use super::{BodyPlacement, EngineeringUnits, Hit, QueryBudget, QueryLimits, Ray, cell};
use crate::{
    World,
    identity::{CampaignId, ReferenceId},
};
use fallout_data::{
    identity::FormKey,
    plugin::{self, Record, RecordHeader},
    record_metadata,
    store::{RecordStore, SourceReceipt},
    vfs::AssetPath,
    world::{
        self,
        dependencies::Decoded,
        residency::{CellResidency, Readiness, Ticket},
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Cell(#[from] cell::CellError),
    #[error(transparent)]
    Source(#[from] fallout_data::Error),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error("reference collision admission: {0}")]
    Invalid(&'static str),
    #[error("reference collision budget exceeded: {0}")]
    Budget(&'static str),
}

#[derive(Clone, Debug)]
pub struct ReferencePlacement {
    pub authored: FormKey,
    pub body: BodyPlacement,
}

/// Aggregate retained-plan, source and binding bounds; scene limits are unchanged.
/// Values can be lowered, never raised above these engineering ceilings.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub scene: QueryLimits,
    pub placements: usize,
    pub sources: usize,
    pub source_bytes: u64,
    pub visits: usize,
    pub record_bytes: usize,
    pub decoded_bytes: usize,
    pub field_sites: usize,
    pub metadata_bytes: usize,
    pub model_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            scene: QueryLimits::default(),
            placements: 1024,
            sources: 256,
            source_bytes: 4 * 1024 * 1024 * 1024,
            visits: 4_000_000,
            record_bytes: 4 * 1024 * 1024,
            decoded_bytes: 64 * 1024 * 1024,
            field_sites: 262144,
            metadata_bytes: 64 * 1024 * 1024,
            model_bytes: 64 * 1024 * 1024,
        }
    }
}
impl Limits {
    fn validate(self) -> Result<Self> {
        let ceiling = Self::default();
        for (value, maximum) in [
            (self.placements, ceiling.placements),
            (self.sources, ceiling.sources),
            (self.visits, ceiling.visits),
            (self.record_bytes, ceiling.record_bytes),
            (self.decoded_bytes, ceiling.decoded_bytes),
            (self.field_sites, ceiling.field_sites),
            (self.metadata_bytes, ceiling.metadata_bytes),
            (self.model_bytes, ceiling.model_bytes),
            (self.scene.blocks, ceiling.scene.blocks),
            (self.scene.shape_visits, ceiling.scene.shape_visits),
            (self.scene.primitives, ceiling.scene.primitives),
            (
                self.scene.geometry_elements,
                ceiling.scene.geometry_elements,
            ),
        ] {
            if value > maximum {
                return Err(Error::Budget("limit ceiling"));
            }
        }
        if self.source_bytes > ceiling.source_bytes {
            return Err(Error::Budget("source byte ceiling"));
        }
        Ok(self)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub reference_checks: usize,
    pub geometry: QueryBudget,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            reference_checks: 2048,
            geometry: QueryBudget::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Definition {
    pub key: FormKey,
    pub source_plugin: String,
    pub source_sha256: String,
    pub header: RecordHeader,
    pub decoded_sha256: String,
}
#[derive(Clone, Debug, Serialize)]
pub struct Binding {
    pub reference: ReferenceId,
    pub body_block: u32,
    pub placed: Definition,
    pub base: Definition,
    pub name_decoded_offset: usize,
    pub model_decoded_offset: usize,
    pub model_path: AssetPath,
    pub attachment_rows: [[f64; 4]; 3],
}
/// Diagnostic only. No API accepts this public observation as authority.
#[derive(Clone, Debug, Serialize)]
pub struct Scope {
    pub campaign: CampaignId,
    pub catalogue_sha256: String,
    pub revision: u64,
    pub cell: cell::Scope,
    pub bindings: Vec<Binding>,
    pub usage: Usage,
}

#[derive(Clone, Debug, Serialize)]
pub struct Usage {
    pub source_bytes: u64,
    pub visits: usize,
    pub decoded_bytes: usize,
    pub field_sites: usize,
    pub metadata_bytes: usize,
}

struct Authority {
    scope: Scope,
    epoch: u64,
    live: AtomicBool,
}
impl Authority {
    fn check(&self, world: &World<'_>, checks: &mut usize) -> Result<()> {
        if !self.live.load(Ordering::Acquire) {
            return Err(Error::Invalid("reference selection lease ended"));
        }
        if world.epoch != self.epoch
            || world.campaign() != self.scope.campaign
            || world.catalogue_fingerprint() != self.scope.catalogue_sha256
            || world.revision() != self.scope.revision
        {
            return Err(Error::Invalid("canonical World or revision changed"));
        }
        charge(checks, self.scope.bindings.len(), "reference checks")?;
        for binding in &self.scope.bindings {
            if world.reference_origin(binding.reference)? != Some(&binding.placed.key)
                || world.authored_reference(&binding.placed.key) != Some(binding.reference)
            {
                return Err(Error::Invalid("canonical reference mapping changed"));
            }
        }
        Ok(())
    }
}

/// Protected source handles cannot be dropped while these results remain usable.
/// Geometry/source-plan pins remain in the original CellCollision/residency owner.
pub struct ReferenceHits<'source> {
    authority: Arc<Authority>,
    hits: cell::CellHits,
    _source_guard: &'source RecordStore,
}
impl ReferenceHits<'_> {
    pub fn scope<'a>(
        &'a self,
        world: &'a World<'_>,
        residency: &'a CellResidency,
    ) -> Result<&'a Scope> {
        let mut checks = self.authority.scope.bindings.len();
        self.authority.check(world, &mut checks)?;
        self.hits.scope(residency)?;
        Ok(&self.authority.scope)
    }
    pub fn hits<'a>(
        &'a self,
        world: &'a World<'_>,
        residency: &'a CellResidency,
    ) -> Result<&'a [Hit]> {
        let mut checks = self.authority.scope.bindings.len();
        self.authority.check(world, &mut checks)?;
        Ok(self.hits.hits(residency)?)
    }
}

fn charge(remaining: &mut usize, count: usize, name: &'static str) -> Result<()> {
    *remaining = remaining.checked_sub(count).ok_or(Error::Budget(name))?;
    Ok(())
}
fn same_sources(a: &[SourceReceipt], b: &[SourceReceipt]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
}
fn one<'a, T>(mut values: impl Iterator<Item = &'a T>) -> Result<&'a T> {
    let value = values
        .next()
        .ok_or(Error::Invalid("missing source association"))?;
    if values.next().is_some() {
        return Err(Error::Invalid("duplicate source association"));
    }
    Ok(value)
}
struct Work {
    visits: usize,
    decoded: usize,
    fields: usize,
    metadata: usize,
    record_max: usize,
}
impl Work {
    fn read(
        &mut self,
        store: &mut RecordStore,
        key: &FormKey,
        header: &RecordHeader,
        digest: &str,
    ) -> Result<Record> {
        charge(&mut self.visits, 1, "source visits")?;
        let location = store
            .winner(key)
            .ok_or(Error::Invalid("missing winning definition"))?;
        if store.definition(location).header != *header || header.flags & plugin::DELETED != 0 {
            return Err(Error::Invalid(
                "winning source definition differs or is deleted",
            ));
        }
        let maximum = self.record_max.min(self.decoded);
        let record = store.read_bounded(location, maximum)?;
        charge(
            &mut self.decoded,
            record.payload.len().max(record.header.stored_size as usize),
            "decoded record bytes",
        )?;
        if record.integrity_issue.is_some()
            || format!("{:x}", Sha256::digest(&record.payload)) != digest
        {
            return Err(Error::Invalid("decoded winning source differs"));
        }
        // Raw buffers are charged by the record budget. Reserve conservative
        // temporary typed-decoder metadata before the existing decoders allocate.
        let scratch = record
            .payload
            .len()
            .checked_mul(16)
            .ok_or(Error::Budget("decode scratch"))?;
        if scratch > self.metadata {
            return Err(Error::Budget("decode scratch"));
        }
        plugin::visit_subrecords(&record, store.source_name(location), |_| {
            charge(&mut self.fields, 1, "field sites")
                .map_err(|e| fallout_data::Error::Resolution(e.to_string()))
        })?;
        Ok(record)
    }
}

/// One selected immutable model, joined to an existing canonical World. This
/// borrows the protected RecordStore so its source handles outlive usable hits.
#[derive(Default)]
pub struct ReferenceCollision<'source> {
    cell: cell::CellCollision,
    authority: Option<Arc<Authority>>,
    source_guard: Option<&'source RecordStore>,
}
impl<'source> ReferenceCollision<'source> {
    #[allow(clippy::too_many_arguments)]
    pub fn admit(
        &mut self,
        world: &World<'_>,
        residency: &mut CellResidency,
        ticket: &Ticket,
        store: &'source mut RecordStore,
        model_index: usize,
        placements: &[ReferencePlacement],
        units: EngineeringUnits,
        limits: Limits,
    ) -> Result<Scope> {
        self.retire_authority();
        self.cell.release(residency)?;
        let sources = residency.sources(ticket).map_err(cell::CellError::from)?;
        residency
            .report_collision(ticket, Readiness::Unsupported)
            .map_err(cell::CellError::from)?;
        let limits = limits.validate()?;
        if placements.is_empty() || placements.len() > limits.placements {
            return Err(Error::Budget("placements"));
        }
        if store.indices().len() > limits.sources {
            return Err(Error::Budget("sources"));
        }
        let mut source_bytes = 0u64;
        let mut work = Work {
            visits: limits.visits,
            decoded: limits.decoded_bytes,
            fields: limits.field_sites,
            metadata: limits.metadata_bytes,
            record_max: limits.record_bytes,
        };
        for index in store.indices() {
            source_bytes = source_bytes
                .checked_add(index.census.source_bytes)
                .ok_or(Error::Budget("source bytes"))?;
            // Existing metadata inspector visits all definitions and winners.
            charge(
                &mut work.visits,
                index
                    .records
                    .len()
                    .checked_mul(2)
                    .ok_or(Error::Budget("source visits"))?,
                "source visits",
            )?;
            charge(
                &mut work.metadata,
                512 + 4 * index.census.name.len(),
                "source receipt metadata",
            )?;
        }
        if source_bytes > limits.source_bytes {
            return Err(Error::Budget("source bytes"));
        }
        if record_metadata::inspect(store)?.winning_definitions_sha256
            != world.catalogue().winning_content_sha256()
        {
            return Err(Error::Invalid(
                "RecordStore winners differ from canonical catalogue",
            ));
        }
        let receipts = store.source_receipts()?;
        let plan = sources.plan().map_err(cell::CellError::from)?;
        let graph = plan.graph();
        // Retaining this shared upstream plan does not multiply its allowances.
        // Include its admitted decode/field/metadata work in this selection cap.
        charge(
            &mut work.decoded,
            plan.receipt().usage.record_decoded_bytes,
            "decoded record bytes",
        )?;
        charge(
            &mut work.fields,
            plan.receipt().usage.field_sites,
            "field sites",
        )?;
        charge(
            &mut work.metadata,
            plan.receipt().usage.metadata_bytes,
            "retained plan metadata",
        )?;
        let upstream_visits = graph
            .usage
            .winners_scanned
            .checked_add(graph.usage.nodes)
            .and_then(|v| v.checked_add(graph.usage.edges))
            .ok_or(Error::Budget("source visits"))?;
        charge(&mut work.visits, upstream_visits, "source visits")?;
        if !same_sources(&receipts, &world.catalogue().sources)
            || !same_sources(&receipts, &graph.sources)
        {
            return Err(Error::Invalid("protected source cohort differs"));
        }
        let model = sources.model(model_index).map_err(cell::CellError::from)?;
        if model.len() > limits.model_bytes {
            return Err(Error::Budget("model bytes"));
        }
        charge(&mut work.decoded, model.len(), "decoded model bytes")?;
        let model_sha: [u8; 32] = Sha256::digest(model).into();
        let request = plan
            .receipt()
            .requests
            .get(model_index)
            .ok_or(Error::Invalid("selected model request missing"))?;
        // Borrowed source graph lookups have a declared aggregate visit bound.
        let per_binding = graph
            .edges
            .len()
            .checked_mul(3)
            .and_then(|n| n.checked_add(graph.nodes.len()))
            .and_then(|n| n.checked_add(plan.receipt().coverage.len()))
            .ok_or(Error::Budget("source visits"))?;
        charge(
            &mut work.visits,
            per_binding
                .checked_mul(placements.len())
                .ok_or(Error::Budget("source visits"))?,
            "source visits",
        )?;
        charge(
            &mut work.metadata,
            512 + 4 * ticket.root().origin_plugin.len(),
            "scope metadata",
        )?;
        let mut bindings = Vec::new();
        let mut bodies = Vec::new();
        let mut unique = BTreeSet::new();
        for placement in placements {
            let body = &placement.body;
            if body.source_sha256 != model_sha
                || world.reference_origin(body.reference)? != Some(&placement.authored)
                || world.authored_reference(&placement.authored) != Some(body.reference)
            {
                return Err(Error::Invalid(
                    "canonical reference/source placement mismatch",
                ));
            }
            charge(
                &mut work.metadata,
                1024 + 4 * placement.authored.origin_plugin.len(),
                "binding metadata",
            )?;
            if !unique.insert((body.reference, body.body_block)) {
                return Err(Error::Invalid("duplicate reference body"));
            }
            one(graph.edges.iter().filter(|e| {
                e.owner == *ticket.root()
                    && e.role == "member"
                    && e.target.status == "resolved"
                    && e.target.key.as_ref() == Some(&placement.authored)
            }))?;
            let node = one(graph.nodes.iter().filter(|n| n.key == placement.authored))?;
            if !matches!(&node.fields, Some(Decoded::Placement(_))) {
                return Err(Error::Invalid("selected source is not a decoded placement"));
            }
            let edge = one(graph
                .edges
                .iter()
                .filter(|e| e.owner == placement.authored && e.role == "NAME"))?;
            if edge.target.status != "resolved" {
                return Err(Error::Invalid("source NAME base is unresolved"));
            }
            let base = edge
                .target
                .key
                .as_ref()
                .ok_or(Error::Invalid("source NAME base missing"))?;
            let coverage = one(plan
                .receipt()
                .coverage
                .iter()
                .filter(|c| &c.base_key == base))?;
            if coverage.asset_path.as_ref() != Some(&request.path) || coverage.candidates.len() != 1
            {
                return Err(Error::Invalid(
                    "reference base does not select this resident model",
                ));
            }
            let candidate = &coverage.candidates[0];
            if candidate.container != request.source.container
                || candidate.entry_index != request.source.entry_index
                || candidate.original_path != request.source.original_path
            {
                return Err(Error::Invalid("model archive association differs"));
            }
            charge(
                &mut work.metadata,
                1024 + 4
                    * (base.origin_plugin.len()
                        + node.source_plugin.len()
                        + coverage.source_plugin.len()
                        + request.path.bytes().len()),
                "binding provenance metadata",
            )?;
            let placed_location = store
                .winner(&placement.authored)
                .ok_or(Error::Invalid("placed winner missing"))?;
            let base_location = store
                .winner(base)
                .ok_or(Error::Invalid("base winner missing"))?;
            if store.source_name(placed_location) != node.source_plugin
                || store.source_name(base_location) != coverage.source_plugin
            {
                return Err(Error::Invalid("winning source plugin differs"));
            }
            let placed_digest = node
                .decoded_sha256
                .as_deref()
                .ok_or(Error::Invalid("placed source digest missing"))?;
            let name_offset = {
                let record = work.read(store, &placement.authored, &node.header, placed_digest)?;
                let typed = world::decode_placement(&record, &node.source_plugin)?;
                if store.key_for(placed_location, typed.base.value)?.as_ref() != Some(base)
                    || typed.base.value != edge.raw_form_id
                {
                    return Err(Error::Invalid("source-owned NAME identity differs"));
                }
                typed.base.decoded_offset
            };
            let model_offset = {
                let record = work.read(store, base, &coverage.header, &coverage.decoded_sha256)?;
                let typed = world::model_path(&record, &coverage.source_plugin)?
                    .ok_or(Error::Invalid("source base lacks MODL"))?;
                let captured = coverage
                    .model_field
                    .as_ref()
                    .ok_or(Error::Invalid("captured MODL missing"))?;
                if typed.decoded_offset != captured.decoded_offset || typed.value != captured.value
                {
                    return Err(Error::Invalid("source-owned MODL differs"));
                }
                typed.decoded_offset
            };
            bindings.push(Binding {
                reference: body.reference,
                body_block: body.body_block,
                placed: Definition {
                    key: placement.authored.clone(),
                    source_plugin: node.source_plugin.clone(),
                    source_sha256: store.source_digest(placed_location)?,
                    header: node.header.clone(),
                    decoded_sha256: placed_digest.to_owned(),
                },
                base: Definition {
                    key: base.clone(),
                    source_plugin: coverage.source_plugin.clone(),
                    source_sha256: store.source_digest(base_location)?,
                    header: coverage.header.clone(),
                    decoded_sha256: coverage.decoded_sha256.clone(),
                },
                name_decoded_offset: name_offset,
                model_decoded_offset: model_offset,
                model_path: request.path.clone(),
                attachment_rows: body.attachment_to_source.rows,
            });
            bodies.push(body.clone());
        }
        let cell = self
            .cell
            .admit(residency, ticket, model_index, &bodies, units, limits.scene)?;
        let scope = Scope {
            campaign: world.campaign(),
            catalogue_sha256: world.catalogue_fingerprint().to_owned(),
            revision: world.revision(),
            cell,
            bindings,
            usage: Usage {
                source_bytes,
                visits: limits.visits - work.visits,
                decoded_bytes: limits.decoded_bytes - work.decoded,
                field_sites: limits.field_sites - work.fields,
                metadata_bytes: limits.metadata_bytes - work.metadata,
            },
        };
        self.authority = Some(Arc::new(Authority {
            scope: scope.clone(),
            epoch: world.epoch,
            live: AtomicBool::new(true),
        }));
        self.source_guard = Some(store);
        Ok(scope)
    }
    fn clear(&mut self) {
        self.retire_authority();
        self.cell = cell::CellCollision::default();
    }
    fn retire_authority(&mut self) {
        if let Some(authority) = self.authority.take() {
            authority.live.store(false, Ordering::Release);
        }
        self.source_guard = None;
    }
    fn current(&mut self, world: &World<'_>, checks: &mut usize) -> Result<Arc<Authority>> {
        let authority = self
            .authority
            .clone()
            .ok_or(Error::Invalid("no admitted reference collision"))?;
        if let Err(error) = authority.check(world, checks) {
            if !matches!(error, Error::Budget(_)) {
                self.clear();
            }
            return Err(error);
        }
        Ok(authority)
    }
    fn finish(
        &mut self,
        world: &World<'_>,
        residency: &CellResidency,
        authority: Arc<Authority>,
        result: cell::CellResult<cell::CellHits>,
        checks: &mut usize,
    ) -> Result<ReferenceHits<'source>> {
        let hits = match result {
            Ok(hits) => hits,
            Err(error) => {
                if !matches!(error, cell::CellError::Query(_)) {
                    self.clear();
                }
                return Err(error.into());
            }
        };
        authority.check(world, checks)?;
        hits.scope(residency)?;
        Ok(ReferenceHits {
            authority,
            hits,
            _source_guard: self
                .source_guard
                .ok_or(Error::Invalid("no protected source guard"))?,
        })
    }
    pub fn ray_cast(
        &mut self,
        world: &World<'_>,
        residency: &CellResidency,
        ray: Ray,
        mut budget: Budget,
    ) -> Result<ReferenceHits<'source>> {
        let authority = self.current(world, &mut budget.reference_checks)?;
        let result = self.cell.ray_cast(residency, ray, budget.geometry);
        self.finish(
            world,
            residency,
            authority,
            result,
            &mut budget.reference_checks,
        )
    }
    pub fn overlap_sphere(
        &mut self,
        world: &World<'_>,
        residency: &CellResidency,
        center: [f64; 3],
        radius: f64,
        mut budget: Budget,
    ) -> Result<ReferenceHits<'source>> {
        let authority = self.current(world, &mut budget.reference_checks)?;
        let result = self
            .cell
            .overlap_sphere(residency, center, radius, budget.geometry);
        self.finish(
            world,
            residency,
            authority,
            result,
            &mut budget.reference_checks,
        )
    }
    pub fn invalidate(&mut self, world: &World<'_>, residency: &CellResidency) -> bool {
        if self.authority.as_ref().is_some_and(|a| {
            let mut checks = a.scope.bindings.len();
            a.check(world, &mut checks).is_err()
        }) || self.cell.invalidate(residency)
        {
            self.clear();
            true
        } else {
            false
        }
    }
    pub fn release(&mut self, residency: &mut CellResidency) -> Result<()> {
        self.retire_authority();
        Ok(self.cell.release(residency)?)
    }
    pub fn retained_primitive_count(&self) -> usize {
        self.cell.retained_primitive_count()
    }
}

impl Drop for ReferenceCollision<'_> {
    fn drop(&mut self) {
        self.retire_authority();
    }
}
