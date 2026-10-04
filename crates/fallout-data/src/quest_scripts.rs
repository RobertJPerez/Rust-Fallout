//! Static quest script attachments and declaration lookup. The authored SCRI
//! relation does not prove which script a live event list currently contains.
use crate::{
    Error, Result,
    identity::FormKey,
    loaded_scripts::{Catalogue, Handle, OwnerKind, ReferenceStatus, ScriptKey},
    narrative, plugin,
    store::{Location, RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    DeletedQuest,
    NoScriptField,
    NullScript,
    MultipleScriptFields,
    MissingScript,
    DeletedScript,
    WrongScriptKind,
    MissingLoadedDefinition,
    MultipleStandaloneUnits,
    LoadedDefinition,
}

#[derive(Debug, Serialize)]
pub struct ScriptField {
    pub decoded_offset: u32,
    pub raw_form: u32,
    pub key: Option<FormKey>,
}
#[derive(Debug, Serialize)]
pub struct Source {
    pub plugin: String,
    pub sha256: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub decoded_record_sha256: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Finding {
    pub field_decoded_offset: u32,
    pub reason: String,
}
#[derive(Debug, Serialize)]
pub struct Attachment {
    pub quest: FormKey,
    pub source: Source,
    pub fields: Vec<ScriptField>,
    pub status: Status,
    pub script: Option<Handle>,
    pub findings: Vec<Finding>,
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub quests: u64,
    pub decoded_bytes: u64,
    pub fields: u64,
    pub source_findings: u64,
    pub statuses: BTreeMap<String, u64>,
}

pub struct Attachments {
    quests: BTreeMap<FormKey, Attachment>,
    source_receipts: Vec<SourceReceipt>,
    pub counts: Counts,
}
impl Attachments {
    /// Complete ordered loader receipts for consumers requiring a full cohort.
    /// The older static declaration join retains its narrower source contract.
    pub fn source_receipts(&self) -> &[SourceReceipt] {
        &self.source_receipts
    }
    pub fn get(&self, key: &FormKey) -> Option<&Attachment> {
        self.quests.get(key)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&FormKey, &Attachment)> {
        self.quests.iter()
    }
    pub fn load(
        store: &mut RecordStore,
        catalogue: &Catalogue,
        maximum_quests: usize,
        mut observe: impl FnMut(usize, &plugin::Record) -> Result<()>,
    ) -> Result<Self> {
        let sources = store.source_receipts()?;
        let expected: BTreeMap<_, _> = catalogue
            .sources
            .iter()
            .map(|source| {
                (
                    source.source_name.to_ascii_lowercase(),
                    &source.source_sha256,
                )
            })
            .collect();
        if sources.len() != expected.len()
            || sources.iter().any(|source| {
                expected.get(&source.source_name.to_ascii_lowercase())
                    != Some(&&source.source_sha256)
            })
        {
            return Err(Error::Resolution(
                "quest attachments and script catalogue have different sources".into(),
            ));
        }
        let mut locations = Vec::new();
        for (key, location) in store.winning_definitions() {
            if store.definition(location).header.kind != *b"QUST" {
                continue;
            }
            if locations.len() >= maximum_quests {
                return Err(Error::Unsupported(
                    "quest attachment record budget exceeded".into(),
                ));
            }
            locations.push((key.clone(), location));
        }
        let mut result = Self {
            quests: BTreeMap::new(),
            source_receipts: Vec::new(),
            counts: Counts::default(),
        };
        for (key, location) in locations {
            let header = &store.definition(location).header;
            let mut attachment = Attachment {
                quest: key.clone(),
                source: Source {
                    plugin: store.source_name(location).into(),
                    sha256: sources[location.plugin].source_sha256.clone(),
                    record_file_offset: header.offset,
                    record_flags: header.flags,
                    decoded_record_sha256: None,
                },
                fields: Vec::new(),
                status: Status::DeletedQuest,
                script: None,
                findings: Vec::new(),
            };
            if header.flags & plugin::DELETED == 0 {
                let record = store.read(location)?;
                result.counts.decoded_bytes += record.payload.len() as u64;
                if result.counts.decoded_bytes > 256 * 1024 * 1024 {
                    return Err(Error::Unsupported(
                        "quest attachment decoded byte budget exceeded".into(),
                    ));
                }
                attachment.source.decoded_record_sha256 =
                    Some(format!("{:x}", Sha256::digest(&record.payload)));
                let document = narrative::decode(
                    &record,
                    store.source_name(location),
                    narrative::Limits::default(),
                )?;
                attachment.findings = document
                    .findings
                    .iter()
                    .map(|finding| Finding {
                        field_decoded_offset: finding.field_offset as u32,
                        reason: finding.reason.into(),
                    })
                    .collect();
                for field in &document.fields {
                    if field.kind != *b"SCRI" {
                        continue;
                    }
                    let narrative::Value::RawForm(raw) = field.value else {
                        return Err(Error::Resolution("quest SCRI lost its form value".into()));
                    };
                    attachment.fields.push(ScriptField {
                        decoded_offset: field.offset as u32,
                        raw_form: raw,
                        key: store.key_for(location, raw)?,
                    });
                }
                resolve(store, catalogue, &mut attachment)?;
                observe(location.plugin, &record)?;
            }
            result.counts.quests += 1;
            result.counts.fields += attachment.fields.len() as u64;
            result.counts.source_findings += attachment.findings.len() as u64;
            let status = serde_json::to_value(attachment.status)
                .expect("status enum")
                .as_str()
                .expect("status string")
                .to_string();
            *result.counts.statuses.entry(status).or_default() += 1;
            result.quests.insert(key, attachment);
        }
        result.source_receipts = sources;
        Ok(result)
    }
}
fn resolve(store: &RecordStore, catalogue: &Catalogue, attachment: &mut Attachment) -> Result<()> {
    if attachment.fields.is_empty() {
        attachment.status = Status::NoScriptField;
        return Ok(());
    }
    if attachment.fields.len() != 1 {
        attachment.status = Status::MultipleScriptFields;
        return Ok(());
    }
    let Some(key) = &attachment.fields[0].key else {
        attachment.status = Status::NullScript;
        return Ok(());
    };
    let Some(location) = store.winner(key) else {
        attachment.status = Status::MissingScript;
        return Ok(());
    };
    let header = &store.definition(location).header;
    if header.flags & plugin::DELETED != 0 {
        attachment.status = Status::DeletedScript;
        return Ok(());
    }
    if header.kind != *b"SCPT" {
        attachment.status = Status::WrongScriptKind;
        return Ok(());
    }
    let mut scripts = catalogue.record_scripts(key);
    let Some(script) = scripts.next() else {
        attachment.status = Status::MissingLoadedDefinition;
        return Ok(());
    };
    if scripts.next().is_some()
        || script.owner().kind != OwnerKind::Standalone
        || !script.owner().schema_ownership_verified
    {
        attachment.status = Status::MultipleStandaloneUnits;
        return Ok(());
    }
    check_source(store, location, script.version())?;
    attachment.status = Status::LoadedDefinition;
    attachment.script = Some(script.handle().clone());
    Ok(())
}
fn check_source(
    store: &RecordStore,
    location: Location,
    version: &crate::loaded_scripts::Version,
) -> Result<()> {
    let header = &store.definition(location).header;
    if store.source_name(location) != version.source_plugin
        || header.offset != version.record_file_offset
        || header.flags != version.record_flags
    {
        return Err(Error::Resolution(
            "quest attachment script is from another winning definition".into(),
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarationStatus {
    StaticQuestDeclaration,
    StaleSourceHandle,
    MissingContextEntry,
    DynamicContext,
    MissingContextVariable,
    RuntimeContext,
    NullContext,
    MissingContextForm,
    DeletedContextForm,
    PlacedReferenceNeedsEventList,
    UnsupportedContextKind,
    MissingQuestAttachment,
    QuestWinnerMismatch,
    QuestScriptUnavailable,
    StaleTargetHandle,
    MissingForeignDeclaration,
}
#[derive(Debug, Serialize)]
pub struct DeclarationLookup {
    pub source_script: ScriptKey,
    pub context_reference: u16,
    pub local_index: u16,
    pub status: DeclarationStatus,
    pub context_form: Option<FormKey>,
    pub quest_attachment_status: Option<Status>,
    pub target_script: Option<Handle>,
    pub declaration: Option<crate::loaded_scripts::Declaration>,
    pub live_value_resolved: bool,
}
pub fn declaration(
    catalogue: &Catalogue,
    attachments: &Attachments,
    source: &Handle,
    context: u16,
    index: u16,
) -> DeclarationLookup {
    let mut row = DeclarationLookup {
        source_script: source.key.clone(),
        context_reference: context,
        local_index: index,
        status: DeclarationStatus::StaleSourceHandle,
        context_form: None,
        quest_attachment_status: None,
        target_script: None,
        declaration: None,
        live_value_resolved: false,
    };
    let Some(script) = catalogue.get_handle(source) else {
        return row;
    };
    let Some(reference) = script.reference(u32::from(context)) else {
        row.status = DeclarationStatus::MissingContextEntry;
        return row;
    };
    row.context_form = reference.form_key.clone();
    row.status = match reference.status {
        ReferenceStatus::DynamicVariable => DeclarationStatus::DynamicContext,
        ReferenceStatus::MissingVariableDeclaration => DeclarationStatus::MissingContextVariable,
        ReferenceStatus::RuntimeDependency => DeclarationStatus::RuntimeContext,
        ReferenceStatus::NullForm => DeclarationStatus::NullContext,
        ReferenceStatus::MissingForm => DeclarationStatus::MissingContextForm,
        ReferenceStatus::DeletedForm => DeclarationStatus::DeletedContextForm,
        ReferenceStatus::DefinedForm => {
            let target = reference.target.as_ref().expect("defined reference header");
            if matches!(
                target.record_kind.as_str(),
                "REFR" | "ACHR" | "ACRE" | "PGRE" | "PMIS" | "PBEA"
            ) {
                DeclarationStatus::PlacedReferenceNeedsEventList
            } else if target.record_kind != "QUST" {
                DeclarationStatus::UnsupportedContextKind
            } else {
                return quest_declaration(catalogue, attachments, target, row);
            }
        }
    };
    row
}
fn quest_declaration(
    catalogue: &Catalogue,
    attachments: &Attachments,
    target: &crate::loaded_scripts::Target,
    mut row: DeclarationLookup,
) -> DeclarationLookup {
    let Some(quest) = attachments.get(row.context_form.as_ref().expect("defined quest key")) else {
        row.status = DeclarationStatus::MissingQuestAttachment;
        return row;
    };
    row.quest_attachment_status = Some(quest.status);
    let source_matches = catalogue.sources.iter().any(|source| {
        source.source_name == quest.source.plugin && source.source_sha256 == quest.source.sha256
    });
    if !source_matches
        || target.source_plugin != quest.source.plugin
        || target.record_file_offset != quest.source.record_file_offset
        || target.record_flags != quest.source.record_flags
    {
        row.status = DeclarationStatus::QuestWinnerMismatch;
        return row;
    }
    let Some(handle) = &quest.script else {
        row.status = DeclarationStatus::QuestScriptUnavailable;
        return row;
    };
    row.target_script = Some(handle.clone());
    let Some(script) = catalogue.get_handle(handle) else {
        row.status = DeclarationStatus::StaleTargetHandle;
        return row;
    };
    row.declaration = script.declaration(u32::from(row.local_index)).cloned();
    row.status = if row.declaration.is_some() {
        DeclarationStatus::StaticQuestDeclaration
    } else {
        DeclarationStatus::MissingForeignDeclaration
    };
    row
}
