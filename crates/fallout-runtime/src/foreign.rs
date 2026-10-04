//! Foreign locals belong to the target's live event list. Authored quest SCRI
//! and base-object script links never replace a missing running instance.
use crate::{
    Error, World,
    identity::{CampaignId, InstanceId, Owner, ReferenceId, ReferenceValue, Value},
    schema::Local,
    state::InstanceHandle,
};
use fallout_data::{
    content,
    identity::FormKey,
    loaded_scripts::{Catalogue, Handle},
    plugin, record_metadata,
    store::RecordStore,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub type Result<T> = std::result::Result<T, Failure>;

#[derive(Debug, thiserror::Error)]
pub enum Failure {
    #[error("foreign context and world have different content cohorts")]
    ContentChanged,
    #[error("foreign context is null")]
    NullContext,
    #[error("foreign context form is missing: {0:?}")]
    MissingForm(FormKey),
    #[error("foreign context form is deleted: {0:?}")]
    DeletedForm(FormKey),
    #[error("foreign context is not a quest or placed reference: {key:?} ({kind:?})")]
    UnsupportedForm { key: FormKey, kind: [u8; 4] },
    #[error("authored reference has no registered live identity: {0:?}")]
    ReferenceNotRegistered(FormKey),
    #[error("foreign context owner has no live script instance: {0:?}")]
    MissingEventList(Owner),
    #[error(transparent)]
    State(#[from] Error),
}
impl Failure {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ContentChanged => "content_changed",
            Self::NullContext => "null_context",
            Self::MissingForm(_) => "missing_form",
            Self::DeletedForm(_) => "deleted_form",
            Self::UnsupportedForm { .. } => "unsupported_form_kind",
            Self::ReferenceNotRegistered(_) => "reference_not_registered",
            Self::MissingEventList(_) => "missing_live_event_list",
            Self::State(Error::MissingLocal(_)) => "missing_local",
            Self::State(Error::UninitializedLocal(_)) => "uninitialized_local",
            Self::State(Error::IncompatibleLocal(_)) => "incompatible_local",
            Self::State(Error::UnsupportedLocal(_)) => "unsupported_local",
            Self::State(Error::StaleHandle) => "stale_handle",
            Self::State(Error::MissingReference) => "missing_live_reference",
            Self::State(Error::UnresolvedDependency(_)) => "unresolved_context_reference",
            Self::State(_) => "invalid_runtime_state",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SourceForm {
    pub kind: [u8; 4],
    pub flags: u32,
}
impl SourceForm {
    /// The existing header classification only. This does not establish source
    /// cell membership, runtime initialization or authored actor behavior.
    pub fn is_placed(&self) -> bool {
        class(&self.kind) == 2
    }
}
fn class(kind: &[u8; 4]) -> u8 {
    match kind {
        b"QUST" => 1,
        b"REFR" | b"ACHR" | b"ACRE" | b"PGRE" | b"PMIS" | b"PBEA" => 2,
        _ => 3,
    }
}
#[derive(Debug, Serialize)]
pub struct Counts {
    pub forms: usize,
    pub quests: usize,
    pub placed: usize,
    pub other: usize,
    pub deleted: usize,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub counts: Counts,
    pub classification_sha256: String,
    pub winning_headers_sha256: String,
}

/// Derived immutable lookup data, rebuilt from source headers after loading.
/// Neither this index nor transient slot handles belong in a native snapshot.
pub struct Content {
    cohort: String,
    forms: BTreeMap<FormKey, SourceForm>,
    report: Report,
}
impl Content {
    pub fn load(
        store: &mut RecordStore,
        catalogue: &Catalogue,
        maximum_forms: usize,
    ) -> crate::Result<Self> {
        let sources = store
            .source_receipts()
            .map_err(|e| Error::Invalid(e.to_string()))?;
        // Include whole-source hashes as well as winners. Equal headers alone
        // would miss a changed body outside the script catalogue.
        let expected = catalogue
            .sources
            .iter()
            .map(|row| {
                (
                    row.source_name.to_ascii_lowercase(),
                    (row.source_bytes, row.source_sha256.clone()),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let actual = sources
            .iter()
            .map(|row| {
                (
                    row.source_name.to_ascii_lowercase(),
                    (row.source_bytes, row.source_sha256.clone()),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let headers = record_metadata::inspect(store).map_err(|e| Error::Invalid(e.to_string()))?;
        if actual != expected
            || headers.winning_definitions_sha256 != catalogue.winning_content_sha256()
        {
            return Err(Error::Invalid(
                "foreign context sources differ from the catalogue".into(),
            ));
        }
        let mut forms = BTreeMap::new();
        let mut counts = Counts {
            forms: 0,
            quests: 0,
            placed: 0,
            other: 0,
            deleted: 0,
        };
        let mut hash = Sha256::new();
        hash.update(b"FRCONTEXT1");
        for (key, location) in store.winning_definitions() {
            if forms.len() >= maximum_forms {
                return Err(Error::Capacity("foreign context forms"));
            }
            let header = &store.definition(location).header;
            let classification = class(&header.kind);
            hash.update((key.origin_plugin.len() as u16).to_le_bytes());
            hash.update(key.origin_plugin.as_bytes());
            hash.update(key.local_id.to_le_bytes());
            hash.update(header.kind);
            hash.update(header.flags.to_le_bytes());
            hash.update([classification]);
            counts.forms += 1;
            counts.deleted += usize::from(header.flags & plugin::DELETED != 0);
            match classification {
                1 => counts.quests += 1,
                2 => counts.placed += 1,
                _ => counts.other += 1,
            }
            forms.insert(
                key.clone(),
                SourceForm {
                    kind: header.kind,
                    flags: header.flags,
                },
            );
        }
        Ok(Self {
            cohort: crate::snapshot::cohort(catalogue)?,
            forms,
            report: Report {
                counts,
                classification_sha256: format!("{:x}", hash.finalize()),
                winning_headers_sha256: headers.winning_definitions_sha256,
            },
        })
    }
    pub fn report(&self) -> &Report {
        &self.report
    }
    /// A probe may contain only local operands, so validate the whole cohort
    /// independently of whether any particular foreign lookup occurs.
    pub fn validate_world(&self, world: &World<'_>) -> Result<()> {
        if self.cohort != world.cohort {
            return Err(Failure::ContentChanged);
        }
        Ok(())
    }
    /// Shared immutable header facts. This does not read deferred record bodies
    /// or invent a runtime binding for a form absent from the source headers.
    pub fn source_form(&self, world: &World<'_>, key: &FormKey) -> Result<SourceForm> {
        self.validate_world(world)?;
        crate::identity::valid_form(key)?;
        Ok(*self.form(key)?)
    }
    fn form(&self, key: &FormKey) -> Result<&SourceForm> {
        let form = self
            .forms
            .get(key)
            .ok_or_else(|| Failure::MissingForm(key.clone()))?;
        if form.flags & plugin::DELETED != 0 {
            return Err(Failure::DeletedForm(key.clone()));
        }
        Ok(form)
    }
    fn placed(&self, key: &FormKey) -> Result<()> {
        if !self.forms.contains_key(key) && content::runtime_binding(key).is_some() {
            return Ok(());
        }
        let form = self.form(key)?;
        if class(&form.kind) != 2 {
            return Err(Failure::UnsupportedForm {
                key: key.clone(),
                kind: form.kind,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Target {
    pub campaign: CampaignId,
    pub state_revision: u64,
    pub source_instance: InstanceId,
    pub context_reference: u16,
    pub resolved_context: ReferenceValue,
    pub target_owner: Owner,
    pub target_instance: InstanceId,
    pub target_definition: Handle,
    pub declaration: Local,
}
#[derive(Debug, Serialize)]
pub struct Read {
    pub target: Target,
    pub value: Value,
}

#[derive(Debug, Clone, Copy)]
pub struct Request {
    pub source: InstanceHandle,
    pub context_reference: u16,
    pub local_index: u16,
    pub player: Option<ReferenceId>,
}

impl World<'_> {
    fn foreign_owner(
        &self,
        content: &Content,
        context: &ReferenceValue,
        player: Option<ReferenceId>,
    ) -> Result<Owner> {
        match context {
            ReferenceValue::Null => Err(Failure::NullContext),
            ReferenceValue::Live { id } => {
                if let Some(key) = self.reference_origin(*id)? {
                    content.placed(key)?;
                }
                Ok(Owner::Placed { reference: *id })
            }
            ReferenceValue::Content { key } => {
                if !content.forms.contains_key(key) && content::runtime_binding(key).is_some() {
                    let id = player
                        .ok_or_else(|| Error::UnresolvedDependency("player reference".into()))?;
                    return self.foreign_owner(content, &ReferenceValue::Live { id }, None);
                }
                let form = content.form(key)?;
                match class(&form.kind) {
                    1 => Ok(Owner::Quest { key: key.clone() }),
                    2 => Ok(Owner::Placed {
                        reference: self
                            .authored_reference(key)
                            .ok_or_else(|| Failure::ReferenceNotRegistered(key.clone()))?,
                    }),
                    _ => Err(Failure::UnsupportedForm {
                        key: key.clone(),
                        kind: form.kind,
                    }),
                }
            }
        }
    }
    pub fn foreign_target(&self, content: &Content, request: Request) -> Result<Target> {
        let Request {
            source,
            context_reference,
            local_index: index,
            player,
        } = request;
        if content.cohort != self.cohort {
            return Err(Failure::ContentChanged);
        }
        let source_instance = self.instance(source)?.id();
        let context =
            self.resolve_script_reference(source, u32::from(context_reference), player)?;
        let owner = self.foreign_owner(content, &context, player)?;
        let target_id = self
            .owner_instance(&owner)
            .ok_or_else(|| Failure::MissingEventList(owner.clone()))?;
        let instance = self.instance(self.handle(target_id)?)?;
        let declaration = instance
            .definition_schema
            .locals
            .get(&u32::from(index))
            .ok_or(Error::MissingLocal(u32::from(index)))?;
        Ok(Target {
            campaign: self.campaign,
            state_revision: self.revision,
            source_instance,
            context_reference,
            resolved_context: context,
            target_owner: owner,
            target_instance: target_id,
            target_definition: instance.definition().clone(),
            declaration: declaration.clone(),
        })
    }
    pub fn read_foreign(&self, content: &Content, request: Request) -> Result<Read> {
        let target = self.foreign_target(content, request)?;
        let value = self
            .instance(self.handle(target.target_instance)?)?
            .local(u32::from(request.local_index))?
            .clone();
        Ok(Read { target, value })
    }
    /// Resolve again under this exclusive world borrow. A previously reported
    /// target is evidence, never authority to mutate a recycled or changed list.
    pub fn assign_foreign(
        &mut self,
        content: &Content,
        request: Request,
        value: Value,
    ) -> Result<Target> {
        let target = self.foreign_target(content, request)?;
        self.assign(
            self.handle(target.target_instance)?,
            &[(u32::from(request.local_index), value)],
        )?;
        Ok(target)
    }
}
