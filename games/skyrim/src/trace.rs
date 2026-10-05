//! Physical source provenance for absent scripts. No active load-order decisions.
use crate::{
    Error, Result,
    plugin::{self, Observation},
    vmad,
};
use fallout_data::{
    baseline::{digest_reader, open_source},
    identity::plugin_name,
    plugin::{self as framing, Group, Record, RecordHeader, SelectedEvent},
    vfs::AssetPath,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::{BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const MAX_ROWS: usize = 100_000;

#[derive(Deserialize)]
struct Input {
    schema_version: u32,
    data: PathBuf,
    plugins: Vec<InputPlugin>,
    script_availability: Vec<InputScript>,
}
#[derive(Deserialize)]
struct InputPlugin {
    file: String,
    bytes: u64,
    sha256: String,
}
#[derive(Deserialize)]
struct InputScript {
    path: Vec<u8>,
    containers: Vec<String>,
}

/// A conservative source-file join key, never a shared runtime FormKey.
/// Noncanonical master selectors stay unresolved instead of adopting NV fallback.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct SourceKey {
    pub origin_plugin: String,
    pub local_id: u32,
}

pub fn source_key(file: &str, masters: &[String], raw: u32) -> Result<Option<SourceKey>> {
    if raw == 0 {
        return Ok(None);
    }
    let index = (raw >> 24) as usize;
    if index > masters.len() {
        return Ok(None);
    }
    Ok(Some(SourceKey {
        origin_plugin: plugin_name(if index == masters.len() {
            file
        } else {
            &masters[index]
        })?,
        local_id: raw & 0xFFFFFF,
    }))
}

/// Identify a physical record owned by its current light plugin from the
/// plugin header alone. Cross-file FE references still require an active light
/// slot map and intentionally remain unresolved by `source_key`.
pub fn source_record_key(
    file: &str,
    masters: &[String],
    raw: u32,
    light: bool,
    header_version_bits: u32,
) -> Result<Option<SourceKey>> {
    if raw >> 24 == 0xFE && light {
        let local_id = raw & 0x00FF_FFFF;
        if !plugin::valid_light_local_id(f32::from_bits(header_version_bits), local_id) {
            return Ok(None);
        }
        return Ok(Some(SourceKey {
            origin_plugin: plugin_name(file)?,
            local_id,
        }));
    }
    source_key(file, masters, raw)
}

#[derive(Debug, Clone, Serialize)]
pub struct Context {
    pub source_plugin: String,
    pub header: RecordHeader,
    pub editor_id: Option<Vec<u8>>,
    pub source_key: Option<SourceKey>,
    pub containing_cell_raw: Option<u32>,
    pub containing_cell: Option<SourceKey>,
    pub containing_world_raw: Option<u32>,
    pub containing_world: Option<SourceKey>,
    pub deleted: bool,
    /// Meaningful only for placed references; absent on other record types.
    pub initially_disabled: Option<bool>,
    pub cell_flags_raw: Option<Vec<u8>>,
    pub vmad_present: bool,
}
impl Context {
    pub(crate) fn resolve(
        &mut self,
        masters: &[String],
        light: bool,
        header_version_bits: u32,
    ) -> Result<()> {
        self.source_key = source_record_key(
            &self.source_plugin,
            masters,
            self.header.form_id,
            light,
            header_version_bits,
        )?;
        self.containing_cell = self
            .containing_cell_raw
            .map(|id| {
                source_record_key(&self.source_plugin, masters, id, light, header_version_bits)
            })
            .transpose()?
            .flatten();
        self.containing_world = self
            .containing_world_raw
            .map(|id| {
                source_record_key(&self.source_plugin, masters, id, light, header_version_bits)
            })
            .transpose()?
            .flatten();
        Ok(())
    }
}

#[derive(Debug, Serialize)]
pub struct AttachmentUse {
    pub record: Context,
    pub asset_path: Vec<u8>,
    pub script_name: Vec<u8>,
    pub attachment_kind: &'static str,
    pub status: Option<u8>,
    pub vmad_subrecord_offset: usize,
    pub binding_offset: usize,
    pub function_name: Option<Vec<u8>>,
}
#[derive(Debug, Serialize)]
pub struct Edge {
    pub record: Context,
    pub field: String,
    pub field_offset: usize,
    pub field_bytes: Vec<u8>,
    pub target_raw: u32,
    pub target: SourceKey,
}
#[derive(Debug, Serialize)]
pub struct Source {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    pub masters: Vec<String>,
    pub light: bool,
    pub header_version_bits: u32,
    pub records: u64,
    pub census_findings: u64,
}
#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub data: PathBuf,
    pub census_sha256: String,
    pub sources: Vec<Source>,
    pub missing_script_paths: Vec<Vec<u8>>,
    pub unlocated_script_paths: Vec<Vec<u8>>,
    pub attachments: Vec<AttachmentUse>,
    pub definition_candidates: Vec<Context>,
    pub direct_references: Vec<Edge>,
    pub containing_cell_candidates: Vec<Context>,
    pub containing_world_candidates: Vec<Context>,
    pub actor_paths: ActorPaths,
    pub unresolved_selected_record_keys: u64,
    pub unresolved_link_indices: u64,
    pub vmad_failures: u64,
    pub limitations: Vec<&'static str>,
}

/// Source-only reverse paths from missing-script magic effects through Skyrim
/// actor spell lists, actor templates, leveled lists, and placed actors. This
/// reports authored references; it does not apply runtime inheritance or roll
/// a leveled list.
#[derive(Debug, Serialize)]
pub struct ActorPaths {
    pub missing_script_effects: Vec<Context>,
    pub spell_candidates: Vec<Context>,
    pub actor_definitions: Vec<Context>,
    pub links: Vec<ActorLink>,
    pub unresolved_links: Vec<ActorLinkFinding>,
    pub unresolved_source_record_keys: u64,
    pub limitations: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActorLink {
    pub record: Context,
    pub field: String,
    pub field_offset: usize,
    pub field_bytes: Vec<u8>,
    pub target_raw: u32,
    pub target: SourceKey,
}

#[derive(Debug, Serialize)]
pub struct ActorLinkFinding {
    pub record: Context,
    pub field: String,
    pub field_offset: usize,
    pub field_bytes: Vec<u8>,
    pub reason: &'static str,
}

#[derive(Default)]
pub(crate) struct Ancestry {
    stack: Vec<Group>,
}
impl Ancestry {
    pub(crate) fn advance(&mut self, offset: u64) {
        while self
            .stack
            .last()
            .is_some_and(|g| offset >= g.offset + u64::from(g.size))
        {
            self.stack.pop();
        }
    }
    pub(crate) fn group(&mut self, group: &Group) {
        self.advance(group.offset);
        self.stack.push(group.clone());
    }
    fn label(&self, kind: i32) -> Option<u32> {
        group_label(&self.stack, kind)
    }
}

pub(crate) fn group_label(groups: &[Group], kind: i32) -> Option<u32> {
    groups
        .iter()
        .rev()
        .find(|group| group.kind == kind)
        .map(|group| u32::from_le_bytes(group.label))
}
fn shared(error: impl std::fmt::Display) -> fallout_data::Error {
    fallout_data::Error::Resolution(error.to_string())
}
fn bounded(len: usize) -> fallout_data::Result<()> {
    if len >= MAX_ROWS {
        Err(shared("trace row budget exceeded"))
    } else {
        Ok(())
    }
}
pub(crate) fn context(
    record: &Record,
    file: &str,
    ancestry: &Ancestry,
) -> fallout_data::Result<Context> {
    let mut result = Context {
        source_plugin: file.into(),
        header: record.header.clone(),
        editor_id: None,
        source_key: None,
        containing_cell_raw: ancestry.label(6),
        containing_cell: None,
        containing_world_raw: ancestry.label(1),
        containing_world: None,
        deleted: record.header.flags & framing::DELETED != 0,
        initially_disabled: matches!(&record.header.kind, b"REFR" | b"ACHR")
            .then_some(record.header.flags & framing::INITIALLY_DISABLED != 0),
        cell_flags_raw: None,
        vmad_present: false,
    };
    framing::visit_subrecords(record, file, |sub| {
        match &sub.kind {
            b"EDID" => {
                if result.editor_id.is_some() {
                    return Err(shared("duplicate trace EDID"));
                }
                result.editor_id = Some(
                    sub.data
                        .strip_suffix(&[0])
                        .ok_or_else(|| shared("unterminated trace EDID"))?
                        .to_vec(),
                );
            }
            b"DATA" if record.header.kind == *b"CELL" => {
                if result.cell_flags_raw.is_some() || !matches!(sub.data.len(), 1 | 2) {
                    return Err(shared("unsupported/duplicate Skyrim CELL flags"));
                }
                result.cell_flags_raw = Some(sub.data.to_vec());
            }
            b"VMAD" => result.vmad_present = true,
            _ => {}
        }
        Ok(())
    })?;
    Ok(result)
}

fn collect_uses(
    attachment: &vmad::RecordAttachment<'_>,
    record: &Record,
    file: &str,
    ancestry: &Ancestry,
    subrecord_offset: usize,
    wanted: &BTreeSet<Vec<u8>>,
    out: &mut Vec<AttachmentUse>,
) -> fallout_data::Result<()> {
    let mut push = |name: &[u8],
                    status: Option<u8>,
                    kind,
                    offset,
                    function: Option<&[u8]>|
     -> fallout_data::Result<()> {
        if status.is_some_and(|s| s & 2 != 0) {
            return Ok(());
        }
        let path = plugin::script_asset_path(name)?;
        if !wanted.contains(path.bytes()) {
            return Ok(());
        }
        bounded(out.len())?;
        out.push(AttachmentUse {
            record: context(record, file, ancestry)?,
            asset_path: path.bytes().to_vec(),
            script_name: name.to_vec(),
            attachment_kind: kind,
            status,
            vmad_subrecord_offset: subrecord_offset,
            binding_offset: offset,
            function_name: function.map(<[u8]>::to_vec),
        });
        Ok(())
    };
    for script in &attachment.primary.scripts {
        push(
            script.name,
            Some(script.status),
            "primary",
            script.offset,
            None,
        )?;
    }
    if let vmad::Tail::Decoded(tail) = &attachment.tail {
        for alias in &tail.aliases {
            for script in &alias.scripts {
                push(
                    script.name,
                    Some(script.status),
                    "quest-alias",
                    script.offset,
                    None,
                )?;
            }
        }
        for fragment in &tail.fragments {
            push(
                fragment.script_name,
                None,
                "fragment",
                fragment.offset,
                Some(fragment.function_name),
            )?;
        }
    }
    Ok(())
}

/// Verify every source hash against the census before publishing any trace.
/// Incoming census master lists are ignored; masters come from the shared parse.
pub fn inspect(census: &Path, mut progress: impl FnMut(&str)) -> Result<Report> {
    let mut input = open_source(census)?;
    if input.metadata()?.len() > 128 * 1024 * 1024 {
        return Err(Error::Unsupported(
            "trace census byte budget exceeded".into(),
        ));
    }
    let (_, census_sha256) = digest_reader(&mut input)?;
    input.seek(SeekFrom::Start(0))?;
    let input: Input = serde_json::from_reader(BufReader::new(input).take(128 * 1024 * 1024))?;
    if !matches!(input.schema_version, 1 | 2)
        || input.plugins.is_empty()
        || input.plugins.len() > 4096
        || input.script_availability.len() > MAX_ROWS
    {
        return Err(Error::Unsupported(
            "trace census version or input budget".into(),
        ));
    }
    let data = input.data.canonicalize()?;
    let mut names = BTreeSet::new();
    for plugin in &input.plugins {
        if !names.insert(plugin_name(&plugin.file)?) {
            return Err(Error::Unsupported("duplicate trace plugin".into()));
        }
    }
    let mut wanted = BTreeSet::new();
    for script in input
        .script_availability
        .iter()
        .filter(|s| s.containers.is_empty())
    {
        let path = AssetPath::new(&script.path)?;
        if !path.bytes().starts_with(b"scripts/") || !path.bytes().ends_with(b".pex") {
            return Err(Error::Unsupported(
                "trace target is not a script asset".into(),
            ));
        }
        wanted.insert(path.bytes().to_vec());
    }
    let mut report = Report {
        schema_version: 2,
        data: data.clone(),
        census_sha256,
        sources: Vec::new(),
        missing_script_paths: wanted.iter().cloned().collect(),
        unlocated_script_paths: Vec::new(),
        attachments: Vec::new(),
        definition_candidates: Vec::new(),
        direct_references: Vec::new(),
        containing_cell_candidates: Vec::new(),
        containing_world_candidates: Vec::new(),
        actor_paths: ActorPaths {
            missing_script_effects: Vec::new(),
            spell_candidates: Vec::new(),
            actor_definitions: Vec::new(),
            links: Vec::new(),
            unresolved_links: Vec::new(),
            unresolved_source_record_keys: 0,
            limitations: vec![
                "Reverse source-reference candidates only; active-plugin winners and runtime actor inheritance are not evaluated",
                "SPLO, TPLT, LVLO, and ACHR NAME bytes are retained; leveled selection, actor initialization, and spell casting are not simulated",
                "File-local source FormIDs with unsupported master selectors remain unresolved; no FE light-slot mapping is inferred",
            ],
        },
        unresolved_selected_record_keys: 0,
        unresolved_link_indices: 0,
        vmad_failures: 0,
        limitations: vec![
            "Physical source occurrences and same-origin definition candidates; no active load order, winners or runtime slot mapping",
            "Strict source master indices; noncanonical IDs stay unresolved and retain their raw bits",
            "Direct edges only: REFR/ACHR NAME and XESP, plus SPEL/SCRL/ALCH/INGR/ENCH EFID; no recursive reachability proof",
            "Missing script targets inherit the census asset inventory; archives are not rescanned by this command",
            "Deleted and initially-disabled flags are source observations; scripts or enable parents can change runtime state",
            "Cell/editor names, including names containing Test, do not prove a record is unreachable",
        ],
    };
    let mut cells = Vec::new();
    let mut worlds = Vec::new();
    let mut actor_definitions = Vec::new();
    let mut actor_links = Vec::new();
    let mut actor_link_findings = Vec::new();
    for expected in &input.plugins {
        progress(&format!("bindings: {}", expected.file));
        let mut ancestry = Ancestry::default();
        let use_start = report.attachments.len();
        let cell_start = cells.len();
        let world_start = worlds.len();
        let parsed = plugin::inspect_with(&data.join(&expected.file), |event| {
            match event {
                Observation::Group(group) => ancestry.group(group),
                Observation::Record(record) => {
                    ancestry.advance(record.header.offset);
                    if matches!(&record.header.kind, b"CELL" | b"WRLD") {
                        let out = if record.header.kind == *b"CELL" {
                            &mut cells
                        } else {
                            &mut worlds
                        };
                        bounded(out.len())?;
                        out.push(context(record, &expected.file, &ancestry)?);
                    }
                }
                Observation::Binding {
                    record,
                    subrecord_offset,
                    attachment,
                } => {
                    collect_uses(
                        attachment,
                        record,
                        &expected.file,
                        &ancestry,
                        subrecord_offset,
                        &wanted,
                        &mut report.attachments,
                    )?;
                }
            }
            Ok(())
        })?;
        if parsed.bytes != expected.bytes || parsed.sha256 != expected.sha256 {
            return Err(Error::Unsupported(format!(
                "source changed since census: {}",
                expected.file
            )));
        }
        for item in &mut report.attachments[use_start..] {
            item.record
                .resolve(&parsed.masters, parsed.light, parsed.header_version_bits)?;
        }
        for item in cells[cell_start..]
            .iter_mut()
            .chain(worlds[world_start..].iter_mut())
        {
            item.resolve(&parsed.masters, parsed.light, parsed.header_version_bits)?;
        }
        report.vmad_failures += parsed.vmad_prefix_failures + parsed.vmad_tail_failures;
        report.sources.push(Source {
            file: parsed.file,
            bytes: parsed.bytes,
            sha256: parsed.sha256,
            masters: parsed.masters,
            light: parsed.light,
            header_version_bits: parsed.header_version_bits,
            records: parsed.records,
            census_findings: parsed.issue_count,
        });
    }
    let observed_paths: BTreeSet<_> = report
        .attachments
        .iter()
        .map(|a| a.asset_path.clone())
        .collect();
    report.unlocated_script_paths = wanted.difference(&observed_paths).cloned().collect();
    let targets: BTreeSet<_> = report
        .attachments
        .iter()
        .filter_map(|a| a.record.source_key.clone())
        .collect();
    report.unresolved_selected_record_keys = report
        .attachments
        .iter()
        .filter(|a| a.record.source_key.is_none())
        .count() as u64;
    for source in &report.sources {
        progress(&format!("references: {}", source.file));
        let mut reader = BufReader::new(open_source(&data.join(&source.file))?);
        let (bytes, hash) = digest_reader(&mut reader)?;
        if bytes != source.bytes || hash != source.sha256 {
            return Err(Error::Unsupported(format!(
                "source changed between trace passes: {}",
                source.file
            )));
        }
        reader.seek(SeekFrom::Start(0))?;
        let mut ancestry = Ancestry::default();
        framing::visit_selected(
            &mut reader,
            bytes,
            &source.file,
            framing::Limits::default(),
            |h| {
                matches!(
                    &h.kind,
                    b"REFR"
                        | b"ACHR"
                        | b"SPEL"
                        | b"SCRL"
                        | b"ALCH"
                        | b"INGR"
                        | b"ENCH"
                        | b"NPC_"
                        | b"LVSP"
                        | b"LVLN"
                ) || source_record_key(
                    &source.file,
                    &source.masters,
                    h.form_id,
                    source.light,
                    source.header_version_bits,
                )
                .ok()
                .flatten()
                .is_some_and(|key| targets.contains(&key))
            },
            |event| {
                let record = match event {
                    SelectedEvent::Group(group) => {
                        ancestry.group(group);
                        return Ok(());
                    }
                    SelectedEvent::Deferred(header) => {
                        ancestry.advance(header.offset);
                        return Ok(());
                    }
                    SelectedEvent::Record(record) => record,
                };
                ancestry.advance(record.header.offset);
                let actor_record =
                    matches!(&record.header.kind, b"NPC_" | b"LVSP" | b"LVLN" | b"ACHR");
                if actor_record {
                    bounded(actor_definitions.len())?;
                    let mut row = context(record, &source.file, &ancestry)?;
                    row.resolve(&source.masters, source.light, source.header_version_bits)
                        .map_err(shared)?;
                    actor_definitions.push(row);
                }
                let key = source_record_key(
                    &source.file,
                    &source.masters,
                    record.header.form_id,
                    source.light,
                    source.header_version_bits,
                )
                .map_err(shared)?;
                if key.as_ref().is_some_and(|key| targets.contains(key)) {
                    bounded(report.definition_candidates.len())?;
                    let mut row = context(record, &source.file, &ancestry)?;
                    row.resolve(&source.masters, source.light, source.header_version_bits)
                        .map_err(shared)?;
                    report.definition_candidates.push(row);
                }
                framing::visit_subrecords(record, &source.file, |sub| {
                    let actor_link = match (&record.header.kind, &sub.kind) {
                        (b"NPC_", b"SPLO") => Some(("SPLO", 0, &[4][..])),
                        (b"NPC_", b"TPLT") => Some(("TPLT", 0, &[4][..])),
                        (b"ACHR", b"NAME") => Some(("NAME", 0, &[4][..])),
                        (b"LVSP" | b"LVLN", b"LVLO") => Some(("LVLO", 4, &[12][..])),
                        _ => None,
                    };
                    if let Some((field, target_offset, allowed_sizes)) = actor_link {
                        let mut row = context(record, &source.file, &ancestry)?;
                        row.resolve(&source.masters, source.light, source.header_version_bits)
                            .map_err(shared)?;
                        if !allowed_sizes.contains(&sub.data.len()) {
                            bounded(actor_link_findings.len())?;
                            actor_link_findings.push(ActorLinkFinding {
                                record: row,
                                field: field.into(),
                                field_offset: sub.payload_offset,
                                field_bytes: sub.data.to_vec(),
                                reason: "unsupported-subrecord-size",
                            });
                            return Ok(());
                        }
                        let target_raw = u32::from_le_bytes(
                            sub.data[target_offset..target_offset + 4]
                                .try_into()
                                .unwrap(),
                        );
                        if target_raw != 0 {
                            if let Some(target) =
                                source_key(&source.file, &source.masters, target_raw)
                                    .map_err(shared)?
                            {
                                bounded(actor_links.len())?;
                                actor_links.push(ActorLink {
                                    record: row,
                                    field: field.into(),
                                    field_offset: sub.payload_offset,
                                    field_bytes: sub.data.to_vec(),
                                    target_raw,
                                    target,
                                });
                            } else {
                                bounded(actor_link_findings.len())?;
                                actor_link_findings.push(ActorLinkFinding {
                                    record: row,
                                    field: field.into(),
                                    field_offset: sub.payload_offset,
                                    field_bytes: sub.data.to_vec(),
                                    reason: "unresolved-source-master-selector",
                                });
                            }
                        }
                    }
                    let placement = matches!(&record.header.kind, b"REFR" | b"ACHR");
                    let magic = matches!(
                        &record.header.kind,
                        b"SPEL" | b"SCRL" | b"ALCH" | b"INGR" | b"ENCH"
                    );
                    let expected =
                        if placement && sub.kind == *b"NAME" || magic && sub.kind == *b"EFID" {
                            4
                        } else if placement && sub.kind == *b"XESP" {
                            8
                        } else {
                            return Ok(());
                        };
                    if sub.data.len() != expected {
                        return Err(shared("unsupported trace link field size"));
                    }
                    let raw = u32::from_le_bytes(sub.data[..4].try_into().unwrap());
                    let Some(target) =
                        source_key(&source.file, &source.masters, raw).map_err(shared)?
                    else {
                        if raw != 0 {
                            report.unresolved_link_indices += 1;
                        }
                        return Ok(());
                    };
                    if targets.contains(&target) {
                        bounded(report.direct_references.len())?;
                        let mut row = context(record, &source.file, &ancestry)?;
                        row.resolve(&source.masters, source.light, source.header_version_bits)
                            .map_err(shared)?;
                        report.direct_references.push(Edge {
                            record: row,
                            field: framing::signature(sub.kind),
                            field_offset: sub.payload_offset,
                            field_bytes: sub.data.to_vec(),
                            target_raw: raw,
                            target,
                        });
                    }
                    Ok(())
                })
            },
        )?;
    }
    let contexts: Vec<_> = report
        .attachments
        .iter()
        .map(|a| &a.record)
        .chain(report.definition_candidates.iter())
        .chain(report.direct_references.iter().map(|e| &e.record))
        .collect();
    let cell_keys: BTreeSet<_> = contexts
        .iter()
        .filter_map(|c| c.containing_cell.clone())
        .collect();
    let world_keys: BTreeSet<_> = contexts
        .iter()
        .filter_map(|c| c.containing_world.clone())
        .collect();
    report.containing_cell_candidates = cells
        .into_iter()
        .filter(|c| c.source_key.as_ref().is_some_and(|k| cell_keys.contains(k)))
        .collect();
    report.containing_world_candidates = worlds
        .into_iter()
        .filter(|c| {
            c.source_key
                .as_ref()
                .is_some_and(|k| world_keys.contains(k))
        })
        .collect();
    report.actor_paths = actor_paths(
        &report.attachments,
        &report.direct_references,
        actor_definitions,
        actor_links,
        actor_link_findings,
    )?;
    Ok(report)
}

fn actor_paths(
    attachments: &[AttachmentUse],
    references: &[Edge],
    definitions: Vec<Context>,
    links: Vec<ActorLink>,
    unresolved_links: Vec<ActorLinkFinding>,
) -> Result<ActorPaths> {
    let effects: BTreeSet<_> = attachments
        .iter()
        .filter(|a| a.record.header.kind == *b"MGEF")
        .filter_map(|a| a.record.source_key.clone())
        .collect();
    let effect_rows: Vec<_> = attachments
        .iter()
        .filter(|a| a.record.header.kind == *b"MGEF" && a.record.source_key.is_some())
        .map(|a| a.record.clone())
        .collect();
    let mut spell_candidates = BTreeMap::new();
    for edge in references
        .iter()
        .filter(|e| e.field == "EFID" && effects.contains(&e.target))
    {
        if let Some(key) = &edge.record.source_key {
            spell_candidates
                .entry(key.clone())
                .or_insert_with(|| edge.record.clone());
        }
    }

    let mut incoming: BTreeMap<SourceKey, Vec<usize>> = BTreeMap::new();
    for (index, edge) in links.iter().enumerate() {
        incoming.entry(edge.target.clone()).or_default().push(index);
    }
    let mut reached: BTreeSet<_> = spell_candidates.keys().cloned().collect();
    let mut queue: VecDeque<_> = reached.iter().cloned().collect();
    let mut selected_edges = BTreeSet::new();
    while let Some(target) = queue.pop_front() {
        if let Some(indices) = incoming.get(&target) {
            for &index in indices {
                selected_edges.insert(index);
                if let Some(source) = links[index].record.source_key.clone()
                    && reached.insert(source.clone())
                {
                    if reached.len() >= MAX_ROWS {
                        return Err(Error::Unsupported(
                            "actor-path reachable-node budget exceeded".into(),
                        ));
                    }
                    queue.push_back(source);
                }
            }
        }
    }
    let actor_definitions = definitions
        .into_iter()
        .filter(|row| {
            row.source_key
                .as_ref()
                .is_some_and(|key| reached.contains(key))
        })
        .collect();
    let unresolved_source_record_keys = selected_edges
        .iter()
        .filter(|index| links[**index].record.source_key.is_none())
        .count() as u64;
    Ok(ActorPaths {
        missing_script_effects: effect_rows,
        spell_candidates: spell_candidates.into_values().collect(),
        actor_definitions,
        links: selected_edges
            .into_iter()
            .map(|index| links[index].clone())
            .collect(),
        unresolved_links,
        unresolved_source_record_keys,
        limitations: vec![
            "Reverse source-reference candidates only; active-plugin winners and runtime actor inheritance are not evaluated",
            "SPLO, TPLT, LVLO, and ACHR NAME bytes are retained; leveled selection, actor initialization, and spell casting are not simulated",
            "File-local source FormIDs with unsupported master selectors remain unresolved; no FE light-slot mapping is inferred",
        ],
    })
}
