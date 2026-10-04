//! Authored actor SCRI links to already admitted standalone script definitions.
//! This module owns no script decoder, event list or runtime instance.
use super::{Catalogue, Definition};
use crate::{
    Error, Result,
    identity::FormKey,
    inventory, loaded_scripts, plugin, record_metadata,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_sources: usize,
    pub max_field_visits: usize,
    pub max_attachments: usize,
    pub max_attachment_bytes: usize,
    pub max_matching_units: usize,
    pub max_declarations: usize,
    pub max_references: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_field_visits: 200_000,
            max_attachments: 4096,
            max_attachment_bytes: 1024 * 1024,
            max_matching_units: 4096,
            max_declarations: 262_144,
            max_references: 1_000_000,
            max_projection_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Attachment {
    pub field_index: usize,
    pub field_decoded_offset: u32,
    pub raw_bytes: Vec<u8>,
    pub binding: Option<inventory::Binding>,
    pub schema_kind_allowed: Option<bool>,
    pub repeated: bool,
}
#[derive(Debug, Serialize)]
pub struct CompiledDefinition<'a> {
    pub handle: &'a loaded_scripts::Handle,
    pub version: &'a loaded_scripts::Version,
    pub owner: &'a loaded_scripts::Owner,
    pub script_type: u16,
    pub flags: u16,
    pub declarations: &'a [loaded_scripts::Declaration],
    pub references: &'a [loaded_scripts::Reference],
    pub issues: &'a [String],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    LoadedDefinition,
    NoScriptField,
    MultipleScriptFields,
    UnsupportedScriptLayout,
    NullScript,
    MissingScript,
    DeletedScript,
    WrongScriptKind,
    MissingLoadedDefinition,
    MultipleStandaloneUnits,
    UnverifiedStandaloneOwner,
    MissingCompiledBody,
    MissingConfiguration,
    MultipleConfigurations,
    UnsupportedConfiguration,
    TemplateInheritanceUnsupported,
}
#[derive(Debug, Serialize)]
pub struct Request<'a> {
    pub sources: &'a [SourceReceipt],
    pub winning_content_sha256: &'a str,
    pub actor: &'a Definition<'a>,
    pub configuration_fields: Vec<&'a inventory::Field>,
    pub template_script_flag: Option<bool>,
    pub attachments: Vec<Attachment>,
    pub compiled_definitions: Vec<CompiledDefinition<'a>>,
    /// A source definition index, never a live instance or event list.
    pub selected_definition: Option<usize>,
    pub status: Status,
    pub field_visits: usize,
    pub attachment_bytes: usize,
    pub declarations: usize,
    pub references: usize,
    pub execution_supported: bool,
    pub scope: &'static str,
}
fn budget(ok: bool, label: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "actor script attachment {label} budget exceeded"
        )))
    }
}
struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
        if raw.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "actor script attachment projection budget",
            ));
        }
        self.bytes += raw.len();
        Ok(raw.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Only the store's existing cursor/digest cache requires a mutable borrow.
/// Definitions, source files and live runtime state remain immutable.
pub fn request<'a>(
    store: &mut RecordStore,
    actors: &'a Catalogue<'a>,
    scripts: &'a loaded_scripts::Catalogue,
    root: &FormKey,
    limits: Limits,
) -> Result<Request<'a>> {
    budget(actors.sources().len() <= limits.max_sources, "source")?;
    budget(scripts.sources.len() <= limits.max_sources, "source")?;
    budget(store.indices().len() <= limits.max_sources, "source")?;
    let receipts = store.source_receipts()?;
    let cohort = serde_json::to_value(&receipts).map_err(|e| Error::Resolution(e.to_string()))?;
    if cohort
        != serde_json::to_value(actors.sources()).map_err(|e| Error::Resolution(e.to_string()))?
        || cohort
            != serde_json::to_value(&scripts.sources)
                .map_err(|e| Error::Resolution(e.to_string()))?
        || actors.winning_content_sha256() != scripts.winning_content_sha256()
        || record_metadata::inspect(store)?.winning_definitions_sha256
            != actors.winning_content_sha256()
    {
        return Err(Error::Resolution(
            "actor script attachment source cohort differs".into(),
        ));
    }
    request_validated(store, actors, scripts, root, limits)
}

fn request_validated<'a>(
    store: &RecordStore,
    actors: &'a Catalogue<'a>,
    scripts: &'a loaded_scripts::Catalogue,
    root: &FormKey,
    limits: Limits,
) -> Result<Request<'a>> {
    let actor = actors
        .get(root)
        .ok_or_else(|| Error::Resolution("actor script attachment root unavailable".into()))?;
    if actor.deleted {
        return Err(Error::Resolution(
            "actor script attachment root deleted".into(),
        ));
    }
    let at = store
        .winner(root)
        .ok_or_else(|| Error::Resolution("actor script attachment winner unavailable".into()))?;
    let record = actor.record().ok_or_else(|| {
        Error::Resolution("actor script attachment retained actor unavailable".into())
    })?;
    let source = actors.sources().get(at.plugin).ok_or_else(|| {
        Error::Resolution("actor script attachment actor source unavailable".into())
    })?;
    if serde_json::to_value(&store.definition(at).header)
        .map_err(|e| Error::Resolution(e.to_string()))?
        != serde_json::to_value(&record.header).map_err(|e| Error::Resolution(e.to_string()))?
        || store.source_name(at) != actor.source.plugin
        || source.source_name != actor.source.plugin
        || source.source_sha256 != actor.source.sha256
        || actor.source.record_file_offset != record.header.offset
        || actor.source.record_flags != record.header.flags
    {
        return Err(Error::Resolution(
            "actor script attachment winning actor differs".into(),
        ));
    }
    let inventory_fields = &actor.inventory_definition().fields;
    let counted_visits = actor
        .fields
        .len()
        .checked_mul(2)
        .and_then(|n| n.checked_add(inventory_fields.len()))
        .ok_or_else(|| Error::Unsupported("actor script attachment field visit overflow".into()))?;
    budget(counted_visits <= limits.max_field_visits, "field visit")?;
    let occurrences = actor.fields.iter().filter(|f| f.kind == *b"SCRI").count();
    budget(occurrences <= limits.max_attachments, "attachment")?;
    let configuration_fields: Vec<_> = inventory_fields
        .iter()
        .filter(|f| f.kind == *b"ACBS")
        .collect();
    let template_script_flag = match configuration_fields.as_slice() {
        [field] => match field.value {
            inventory::Value::ActorBase { template_flags, .. } => Some(template_flags & 512 != 0),
            _ => None,
        },
        _ => None,
    };
    let mut result = Request {
        sources: actors.sources(),
        winning_content_sha256: actors.winning_content_sha256(),
        actor,
        configuration_fields,
        template_script_flag,
        attachments: Vec::new(),
        compiled_definitions: Vec::new(),
        selected_definition: None,
        status: Status::NoScriptField,
        field_visits: counted_visits,
        attachment_bytes: 0,
        declarations: 0,
        references: 0,
        execution_supported: false,
        scope: "Exact authored actor SCRI fields and existing source-bound standalone compiled definitions; source handles/version/owner/declarations/references only, no template inheritance, event-list replacement, live instance, event dispatch or execution",
    };
    let mut physical_visits = 0;
    let mut counts = inventory::Counts::default();
    plugin::visit_subrecords(record, &actor.source.plugin, |field| {
        let index = physical_visits;
        physical_visits += 1;
        let cached = actor.fields.get(index).ok_or_else(|| {
            Error::Resolution("actor script attachment physical field missing".into())
        })?;
        if cached.kind != field.kind
            || cached.decoded_offset as usize != field.payload_offset
            || cached.bytes != field.data.len()
        {
            return Err(Error::Resolution(
                "actor script attachment physical field differs".into(),
            ));
        }
        if field.kind == *b"SCRI" {
            budget(
                field.data.len()
                    <= limits
                        .max_attachment_bytes
                        .saturating_sub(result.attachment_bytes),
                "attachment byte",
            )?;
            result.attachment_bytes += field.data.len();
            let binding = if field.data.len() == 4 {
                Some(inventory::binding(
                    store,
                    at,
                    u32::from_le_bytes(field.data.try_into().expect("four SCRI bytes")),
                    &mut counts,
                )?)
            } else {
                None
            };
            let allowed = binding
                .as_ref()
                .and_then(|b| b.target.as_ref())
                .map(|t| t.kind == *b"SCPT");
            result.attachments.push(Attachment {
                field_index: index,
                field_decoded_offset: cached.decoded_offset,
                raw_bytes: field.data.to_vec(),
                binding,
                schema_kind_allowed: allowed,
                repeated: occurrences > 1,
            });
        }
        Ok(())
    })?;
    if physical_visits != actor.fields.len() {
        return Err(Error::Resolution(
            "actor script attachment field count differs".into(),
        ));
    }
    result.status = resolve(store, scripts, &mut result, limits)?;
    if result.status == Status::LoadedDefinition {
        result.status = match result.configuration_fields.as_slice() {
            [] => Status::MissingConfiguration,
            [_] if result.template_script_flag.is_none() => Status::UnsupportedConfiguration,
            [_] if result.template_script_flag == Some(true) => {
                Status::TemplateInheritanceUnsupported
            }
            [_] => Status::LoadedDefinition,
            _ => Status::MultipleConfigurations,
        };
        if result.status == Status::LoadedDefinition {
            result.selected_definition = Some(0);
        }
    }
    serde_json::to_writer(
        &mut Admission {
            bytes: 0,
            maximum: limits.max_projection_bytes,
        },
        &result,
    )
    .map_err(|e| Error::Unsupported(format!("actor script attachment projection: {e}")))?;
    Ok(result)
}
fn resolve<'a>(
    store: &RecordStore,
    scripts: &'a loaded_scripts::Catalogue,
    result: &mut Request<'a>,
    limits: Limits,
) -> Result<Status> {
    let attachment = match result.attachments.as_slice() {
        [] => return Ok(Status::NoScriptField),
        [field] => field,
        _ => return Ok(Status::MultipleScriptFields),
    };
    let Some(binding) = &attachment.binding else {
        return Ok(Status::UnsupportedScriptLayout);
    };
    match binding.status {
        inventory::Status::Null => return Ok(Status::NullScript),
        inventory::Status::Missing => return Ok(Status::MissingScript),
        inventory::Status::Deleted => return Ok(Status::DeletedScript),
        inventory::Status::Defined => (),
    }
    if attachment.schema_kind_allowed != Some(true) {
        return Ok(Status::WrongScriptKind);
    }
    let key = binding.key.as_ref().expect("defined script key");
    let target = binding.target.as_ref().expect("defined script target");
    for script in scripts.record_scripts(key) {
        budget(
            result.compiled_definitions.len() < limits.max_matching_units,
            "matching unit",
        )?;
        budget(
            script.declarations().len()
                <= limits.max_declarations.saturating_sub(result.declarations),
            "declaration",
        )?;
        budget(
            script.references().len() <= limits.max_references.saturating_sub(result.references),
            "reference",
        )?;
        let version = script.version();
        let source = result
            .sources
            .iter()
            .find(|s| s.source_name == target.source_plugin)
            .ok_or_else(|| {
                Error::Resolution("actor script attachment target source unavailable".into())
            })?;
        if version.source_plugin != target.source_plugin
            || version.source_sha256 != source.source_sha256
            || version.record_file_offset != target.record_file_offset
            || version.record_flags != target.record_flags
            || script.handle().key.record != *key
            || scripts.get_handle(script.handle()).is_none()
            || store.winner(key).is_none()
        {
            return Err(Error::Resolution(
                "actor script attachment loaded winner differs".into(),
            ));
        }
        result.declarations += script.declarations().len();
        result.references += script.references().len();
        result.compiled_definitions.push(CompiledDefinition {
            handle: script.handle(),
            version,
            owner: script.owner(),
            script_type: script.script_type(),
            flags: script.flags(),
            declarations: script.declarations(),
            references: script.references(),
            issues: script.issues(),
        });
    }
    Ok(match result.compiled_definitions.as_slice() {
        [] => Status::MissingLoadedDefinition,
        [script]
            if script.owner.kind != loaded_scripts::OwnerKind::Standalone
                || !script.owner.schema_ownership_verified =>
        {
            Status::UnverifiedStandaloneOwner
        }
        [script] if script.version.compiled_sha256.is_none() => Status::MissingCompiledBody,
        [_] => Status::LoadedDefinition,
        _ => Status::MultipleStandaloneUnits,
    })
}
