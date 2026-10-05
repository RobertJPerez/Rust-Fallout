//! Ticket-bound reference staging for a cell transition.
//!
//! This module prepares canonical state for an existing Host scene-admission
//! boundary. It does not publish the state: consumers must keep the ticket and
//! resident source lease alive and commit the reference proposal inside the
//! existing `CellResidency::publish_render` callback.
use super::{Staged, State};
use crate::{Error, Result, World, identity::ReferenceId};
use fallout_data::world::residency::{CellResidency, ResidentSources, Ticket};
use std::{fmt::Display, sync::Arc};

/// A destination state staged against the exact current World view and a live
/// cell ticket. The retained source lease keeps its source plan/cohort pinned;
/// it does not confer render, collision, behavior, simulation, or commit
/// readiness.
#[must_use = "pass the staged state through Host scene admission or drop it"]
pub struct StagedResidentReference {
    reference: Staged,
    ticket: Ticket,
    sources: Arc<ResidentSources>,
    source_identity: String,
}

impl StagedResidentReference {
    pub fn reference_stage(&self) -> &Staged {
        &self.reference
    }

    pub fn ticket(&self) -> &Ticket {
        &self.ticket
    }

    pub fn source_lease(&self) -> &Arc<ResidentSources> {
        &self.sources
    }

    pub fn source_identity(&self) -> &str {
        &self.source_identity
    }

    /// Split the retained inputs for Host admission. Keep the lease alive until
    /// the callback passed to `CellResidency::publish_render` has completed.
    pub fn into_host_admission_parts(self) -> (Ticket, Arc<ResidentSources>, String, Staged) {
        (
            self.ticket,
            self.sources,
            self.source_identity,
            self.reference,
        )
    }
}

fn residency_error(error: impl Display) -> Error {
    Error::Invalid(format!("cell residency ticket rejected: {error}"))
}

impl World<'_> {
    /// Prepare a reference destination for the current decoded residency ticket.
    /// State.cell must exactly match Ticket.root. This method captures the
    /// current reference View and retains the resolved source lease, but does
    /// not authorize or perform a canonical commit.
    pub fn stage_resident_reference_state(
        &self,
        residency: &CellResidency,
        ticket: &Ticket,
        reference: ReferenceId,
        state: State,
    ) -> Result<StagedResidentReference> {
        if state.cell() != ticket.root() {
            return Err(Error::Invalid(
                "reference destination does not match cell residency ticket root".into(),
            ));
        }

        let sources = residency.sources(ticket).map_err(residency_error)?;
        let plan = sources.plan().map_err(residency_error)?;
        if plan.root() != ticket.root()
            || plan.identity() != ticket.identity()
            || sources.ticket().root() != ticket.root()
            || sources.ticket().identity() != ticket.identity()
            || sources.ticket().generation() != ticket.generation()
        {
            return Err(Error::Invalid(
                "cell residency source cohort differs from its ticket".into(),
            ));
        }
        let source_identity = plan.identity().to_owned();
        drop(plan);

        let view = self.reference_view(reference)?;
        let reference = self.stage_reference_state(&view, state)?;
        Ok(StagedResidentReference {
            reference,
            ticket: ticket.clone(),
            sources,
            source_identity,
        })
    }

    /// Revalidate the retained ticket, source cohort/lease, and exact canonical
    /// World view before handing the stage to Host admission. This is a
    /// preflight only: canonical commit still belongs inside the existing
    /// residency publication callback, which owns the ticket cancellation gate.
    pub fn validate_resident_reference_stage(
        &self,
        residency: &CellResidency,
        stage: &StagedResidentReference,
    ) -> Result<()> {
        if stage.reference.state().cell() != stage.ticket.root() {
            return Err(Error::Invalid(
                "reference destination does not match cell residency ticket root".into(),
            ));
        }
        stage.reference.state().validate()?;

        let current = residency.sources(&stage.ticket).map_err(residency_error)?;
        if !Arc::ptr_eq(&current, &stage.sources) {
            return Err(Error::StaleHandle);
        }
        let plan = current.plan().map_err(residency_error)?;
        if plan.root() != stage.ticket.root()
            || plan.identity() != stage.source_identity
            || stage.ticket.identity() != stage.source_identity
            || current.ticket().root() != stage.ticket.root()
            || current.ticket().identity() != stage.ticket.identity()
            || current.ticket().generation() != stage.ticket.generation()
        {
            return Err(Error::Invalid(
                "cell residency source cohort changed after staging".into(),
            ));
        }
        drop(plan);
        self.validate_reference_view(stage.reference.base())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Limits, identity::CampaignId};
    use fallout_data::{
        identity::{FormKey, ProfileId},
        loaded_scripts::{Catalogue, Limits as CatalogueLimits},
        plugin,
        store::RecordStore,
        vfs::MountIndex,
        world::{
            Transform,
            preparation::CellModelPlan,
            residency::{CellResidency, Limits as ResidencyLimits, Stage as ResidencyStage},
        },
    };
    use std::{
        fs,
        path::Path,
        sync::Arc,
        thread,
        time::{Duration, Instant},
    };

    fn form(local_id: u32) -> FormKey {
        FormKey {
            profile: ProfileId::NvOriginal,
            origin_plugin: "falloutnv.esm".into(),
            local_id,
        }
    }

    fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
        [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
    }

    fn record(kind: &[u8; 4], id: u32, body: &[u8]) -> Vec<u8> {
        [
            kind.as_slice(),
            &(body.len() as u32).to_le_bytes(),
            &[0; 4],
            &id.to_le_bytes(),
            &[0; 8],
            body,
        ]
        .concat()
    }

    fn source_fixture(root: &Path) -> (Catalogue, CellModelPlan, CellModelPlan) {
        fs::create_dir_all(root).unwrap();
        let header = record(
            b"TES4",
            0,
            &field(
                b"HEDR",
                &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
            ),
        );
        let cells = [0x400, 0x402]
            .into_iter()
            .map(|id| record(b"CELL", id, &field(b"DATA", &[0])))
            .collect::<Vec<_>>();
        fs::write(
            root.join("FalloutNV.esm"),
            [header, cells.concat()].concat(),
        )
        .unwrap();

        let mut store = RecordStore::open_nv_headers(
            root,
            &["FalloutNV.esm".into()],
            plugin::Limits::default(),
        )
        .unwrap();
        let mounts = MountIndex::default();
        let first =
            CellModelPlan::load(&mut store, &form(0x400), &mounts, Default::default()).unwrap();
        let second =
            CellModelPlan::load(&mut store, &form(0x402), &mounts, Default::default()).unwrap();
        let catalogue =
            Catalogue::load(&mut store, CatalogueLimits::default(), |_, _| Ok(())).unwrap();
        (catalogue, first, second)
    }

    fn state(cell: u32, x: f32) -> State {
        State::new(
            form(cell),
            super::super::Pose::from_source(
                &Transform {
                    position: [x, 0.0, 3.0],
                    rotation: [0.0, 0.0, 0.0],
                },
                Some(1.0),
            )
            .unwrap(),
            true,
        )
        .unwrap()
    }

    fn residency(source: &Path) -> CellResidency {
        CellResidency::new(source, None, ResidencyLimits::default()).unwrap()
    }

    fn decoded(owner: &mut CellResidency) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while owner.poll().unwrap().stage != ResidencyStage::Decoded {
            assert!(Instant::now() < deadline, "cell sources did not decode");
            thread::yield_now();
        }
    }

    fn world<'a>(catalogue: &'a Catalogue) -> (World<'a>, ReferenceId) {
        let mut world = World::with_campaign(
            catalogue,
            Limits::default(),
            CampaignId::from_bytes([0x63; 16]).unwrap(),
        )
        .unwrap();
        let reference = world.register_reference(None).unwrap();
        (world, reference)
    }

    #[test]
    fn resident_stage_binds_ticket_cohort_world_view_and_rejects_foreign_or_mismatched_input() {
        let directory = tempfile::tempdir().unwrap();
        let (catalogue, first_plan, _) = source_fixture(directory.path());
        let mut owner = residency(directory.path());
        let ticket = owner.request(first_plan.clone()).unwrap();
        decoded(&mut owner);
        let (mut world, reference) = world(&catalogue);
        let before = world.snapshot();

        assert!(
            world
                .stage_resident_reference_state(&owner, &ticket, reference, state(0x402, 2.0))
                .is_err()
        );
        assert_eq!(world.snapshot(), before);

        let mut foreign_owner = residency(directory.path());
        let foreign_ticket = foreign_owner.request(first_plan).unwrap();
        decoded(&mut foreign_owner);
        assert!(
            world
                .stage_resident_reference_state(
                    &owner,
                    &foreign_ticket,
                    reference,
                    state(0x400, 3.0),
                )
                .is_err()
        );
        assert_eq!(world.snapshot(), before);

        let stage = world
            .stage_resident_reference_state(&owner, &ticket, reference, state(0x400, 4.0))
            .unwrap();
        assert_eq!(stage.ticket().root(), &form(0x400));
        assert_eq!(stage.source_identity(), ticket.identity());
        assert!(Arc::ptr_eq(
            stage.source_lease(),
            &owner.sources(&ticket).unwrap()
        ));
        assert_eq!(stage.reference_stage().base().revision(), world.revision());
        world
            .validate_resident_reference_stage(&owner, &stage)
            .unwrap();
        assert_eq!(world.snapshot(), before);

        let current_view = world.reference_view(reference).unwrap();
        let intervening = world
            .stage_reference_state(&current_view, state(0x400, 5.0))
            .unwrap();
        world.commit_reference_state(intervening).unwrap();
        let after_intervening = world.snapshot();
        assert!(
            world
                .validate_resident_reference_stage(&owner, &stage)
                .is_err()
        );
        assert_eq!(world.snapshot(), after_intervening);
    }

    #[test]
    fn resident_stage_rejects_replacement_and_unload_while_retaining_the_source_lease() {
        let directory = tempfile::tempdir().unwrap();
        let (catalogue, first_plan, replacement_plan) = source_fixture(directory.path());
        let mut owner = residency(directory.path());
        let first_ticket = owner.request(first_plan).unwrap();
        decoded(&mut owner);
        let (world, reference) = world(&catalogue);
        let stage = world
            .stage_resident_reference_state(&owner, &first_ticket, reference, state(0x400, 1.0))
            .unwrap();

        let replacement = owner.request(replacement_plan).unwrap();
        assert_eq!(replacement.root(), &form(0x402));
        let snapshot = world.snapshot();
        assert!(
            world
                .validate_resident_reference_stage(&owner, &stage)
                .is_err()
        );
        assert_eq!(world.snapshot(), snapshot);
        drop(stage);

        decoded(&mut owner);
        let stage = world
            .stage_resident_reference_state(&owner, &replacement, reference, state(0x402, 2.0))
            .unwrap();
        owner.unload().unwrap();
        let snapshot = world.snapshot();
        assert!(
            world
                .validate_resident_reference_stage(&owner, &stage)
                .is_err()
        );
        assert_eq!(world.snapshot(), snapshot);
        assert_eq!(owner.snapshot().retained_plans, 1);

        drop(stage);
        let deadline = Instant::now() + Duration::from_secs(5);
        while owner.poll().unwrap().stage != ResidencyStage::Unrequested {
            assert!(
                Instant::now() < deadline,
                "retained source lease did not drain"
            );
            thread::yield_now();
        }
        assert_eq!(owner.snapshot().retained_plans, 0);
    }
}
