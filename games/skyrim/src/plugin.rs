//! Skyrim metadata layered on the shared record/subrecord visitor.
use crate::{Result, bad, vmad};
use fallout_data::{
    baseline::{digest_reader, open_source},
    identity::plugin_name,
    plugin::{self, Event},
    vfs::AssetPath,
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{BufReader, Seek, SeekFrom},
    path::Path,
};

#[derive(Debug, Serialize)]
pub struct PluginReport {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    pub header_version_bits: u32,
    pub header_flags: u32,
    /// Light status from the TES4 flag; the file extension is tracked separately.
    pub light: bool,
    pub esl_extension: bool,
    pub localized: bool,
    pub masters: Vec<String>,
    pub records: u64,
    pub groups: u64,
    pub record_types: BTreeMap<String, u64>,
    pub record_versions: BTreeMap<u16, u64>,
    pub subrecord_types: BTreeMap<String, u64>,
    pub vmad_records: u64,
    pub vmad_versions: BTreeMap<u16, u64>,
    pub vmad_object_formats: BTreeMap<u16, u64>,
    pub vmad_properties: u64,
    pub vmad_prefix_failures: u64,
    pub vmad_tails: u64,
    pub vmad_decoded_tails: u64,
    pub vmad_tail_failures: u64,
    pub fragment_kinds: BTreeMap<String, u64>,
    pub quest_aliases: u64,
    pub alias_properties: u64,
    pub vmad_object_index_findings: u64,
    pub issue_count: u64,
    pub issue_examples: Vec<String>,
    pub scripts: Vec<ScriptUse>,
    pub status: &'static str,
}
#[derive(Debug, Serialize)]
pub struct ScriptUse {
    pub name_bytes: Vec<u8>,
    pub asset_path: Option<Vec<u8>>,
    pub attachments: u64,
    pub non_removed_attachments: u64,
    pub status_bytes: BTreeMap<u8, u64>,
    pub alias_attachments: u64,
    pub fragment_references: u64,
    /// Source/debug filename metadata is not itself an executable binding.
    pub fragment_file_hints: u64,
    pub first_form_id: u32,
    pub first_record_offset: u64,
    pub first_record_kind: String,
}

#[derive(Clone, Copy)]
enum UseKind {
    Primary(u8),
    Alias(u8),
    Fragment,
    FragmentFile,
}

/// Observers share the bounded framing and VMAD decode used by the census.
pub enum Observation<'a, 'data> {
    Group(&'a plugin::Group),
    Record(&'a plugin::Record),
    Binding {
        record: &'a plugin::Record,
        subrecord_offset: usize,
        attachment: &'a vmad::RecordAttachment<'data>,
    },
}

pub fn script_asset_path(name: &[u8]) -> fallout_data::Result<AssetPath> {
    let mut path = b"scripts/".to_vec();
    path.extend_from_slice(name);
    path.extend_from_slice(b".pex");
    AssetPath::new(&path)
}

fn track_script(
    scripts: &mut BTreeMap<Vec<u8>, ScriptUse>,
    name: &[u8],
    kind: UseKind,
    header: &plugin::RecordHeader,
) -> fallout_data::Result<()> {
    if !scripts.contains_key(name) && scripts.len() >= 100_000 {
        return Err(fallout_data::Error::Resolution(
            "script-name inventory budget exceeded".into(),
        ));
    }
    let entry = scripts.entry(name.to_vec()).or_insert_with(|| ScriptUse {
        name_bytes: name.to_vec(),
        asset_path: script_asset_path(name).ok().map(|p| p.bytes().to_vec()),
        attachments: 0,
        non_removed_attachments: 0,
        status_bytes: BTreeMap::new(),
        alias_attachments: 0,
        fragment_references: 0,
        fragment_file_hints: 0,
        first_form_id: header.form_id,
        first_record_offset: header.offset,
        first_record_kind: plugin::signature(header.kind),
    });
    match kind {
        UseKind::Primary(status) | UseKind::Alias(status) => {
            entry.attachments += 1;
            *entry.status_bytes.entry(status).or_default() += 1;
            if status & 2 == 0 {
                entry.non_removed_attachments += 1;
            }
            if matches!(kind, UseKind::Alias(_)) {
                entry.alias_attachments += 1;
            }
        }
        UseKind::Fragment => entry.fragment_references += 1,
        UseKind::FragmentFile => entry.fragment_file_hints += 1,
    }
    Ok(())
}
impl PluginReport {
    fn issue(&mut self, message: String) {
        self.issue_count += 1;
        if self.issue_examples.len() < 64 {
            self.issue_examples.push(message);
        }
    }
    fn object(&mut self, object: &vmad::Object, header: &plugin::RecordHeader, at: usize) {
        if (object.form_id >> 24) as usize > self.masters.len() {
            self.vmad_object_index_findings += 1;
            self.issue(format!(
                "VMAD object {:08X} exceeds {} source masters at record {:08X}, file 0x{:X}, VMAD field +0x{at:X}; raw bits retained, runtime meaning unresolved",
                object.form_id, self.masters.len(), header.form_id, header.offset
            ));
        }
    }
    fn value(&mut self, value: &vmad::Value<'_>, header: &plugin::RecordHeader, at: usize) {
        match value {
            vmad::Value::Object(object) => self.object(object, header, at),
            vmad::Value::Array { values, .. } => {
                for value in values {
                    self.value(value, header, at);
                }
            }
            _ => {}
        }
    }
    fn properties(&mut self, script: &vmad::Script<'_>, header: &plugin::RecordHeader) {
        for property in &script.properties {
            self.value(&property.value, header, property.offset);
        }
    }
}
/// Header 1.71 permits the expanded light-plugin local-ID range. This validates
/// file-local NEW records, never masks a runtime FE slot into an on-disk FormID.
pub fn valid_light_local_id(header_version: f32, local_id: u32) -> bool {
    match header_version.to_bits() {
        n if n == 1.7f32.to_bits() => (0x800..=0xFFF).contains(&local_id),
        n if n == 1.71f32.to_bits() => local_id <= 0xFFF,
        _ => false,
    }
}

pub fn inspect(path: &Path) -> Result<PluginReport> {
    inspect_with(path, |_| Ok(()))
}

pub fn inspect_with(
    path: &Path,
    mut observe: impl FnMut(Observation<'_, '_>) -> fallout_data::Result<()>,
) -> Result<PluginReport> {
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| bad("plugin", 0, "non-Unicode filename"))?;
    plugin_name(file_name)?;
    let name = path.display().to_string();
    let source = open_source(path)?;
    let mut reader = BufReader::new(source);
    let (bytes, sha256) = digest_reader(&mut reader)?;
    reader.seek(SeekFrom::Start(0))?;
    let mut report = PluginReport {
        file: file_name.into(),
        bytes,
        sha256,
        header_version_bits: 0,
        header_flags: 0,
        light: false,
        esl_extension: path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("esl")),
        localized: false,
        masters: Vec::new(),
        records: 0,
        groups: 0,
        record_types: BTreeMap::new(),
        record_versions: BTreeMap::new(),
        subrecord_types: BTreeMap::new(),
        vmad_records: 0,
        vmad_versions: BTreeMap::new(),
        vmad_object_formats: BTreeMap::new(),
        vmad_properties: 0,
        vmad_prefix_failures: 0,
        vmad_tails: 0,
        vmad_decoded_tails: 0,
        vmad_tail_failures: 0,
        fragment_kinds: BTreeMap::new(),
        quest_aliases: 0,
        alias_properties: 0,
        vmad_object_index_findings: 0,
        issue_count: 0,
        issue_examples: Vec::new(),
        scripts: Vec::new(),
        status: "record bodies framed; supported VMAD bindings decoded; gameplay unimplemented",
    };
    let mut scripts: BTreeMap<Vec<u8>, ScriptUse> = BTreeMap::new();
    let mut hedr_seen = false;
    let mut master_names = BTreeSet::new();
    plugin::visit(
        &mut reader,
        bytes,
        &name,
        plugin::Limits::default(),
        |event| {
            match &event {
                Event::Group(group) => observe(Observation::Group(group))?,
                Event::Record(record) => observe(Observation::Record(record))?,
            }
            let Event::Record(record) = event else {
                report.groups += 1;
                return Ok(());
            };
            report.records += 1;
            let header = &record.header;
            *report
                .record_types
                .entry(plugin::signature(header.kind))
                .or_default() += 1;
            *report.record_versions.entry(header.version).or_default() += 1;
            if header.kind == *b"TES4" {
                report.header_flags = header.flags;
                report.light = header.flags & 0x200 != 0;
                report.localized = header.flags & 0x80 != 0;
            } else if report.light
                && (header.form_id >> 24) as usize >= report.masters.len()
                && !valid_light_local_id(
                    f32::from_bits(report.header_version_bits),
                    header.form_id & 0xFFFFFF,
                )
            {
                report.issue(format!(
                    "new light-plugin FormID {:08X} outside declared HEDR range at 0x{:X}",
                    header.form_id, header.offset
                ));
            }
            let mut record_vmad_count = 0;
            plugin::visit_subrecords(record, &name, |sub| {
                *report
                    .subrecord_types
                    .entry(plugin::signature(sub.kind))
                    .or_default() += 1;
                if header.kind == *b"TES4" {
                    if sub.kind == *b"HEDR" {
                        if hedr_seen || sub.data.len() != 12 {
                            return Err(fallout_data::Error::Resolution(
                                "missing-size/duplicate HEDR".into(),
                            ));
                        }
                        hedr_seen = true;
                        report.header_version_bits =
                            u32::from_le_bytes(sub.data[..4].try_into().unwrap());
                        if !matches!(report.header_version_bits, n if n == 1.7f32.to_bits() || n == 1.71f32.to_bits())
                        {
                            report.issue(format!(
                                "unverified HEDR version bits {:08X}",
                                report.header_version_bits
                            ));
                        }
                    } else if sub.kind == *b"MAST" {
                        let bytes = sub.data.strip_suffix(&[0]).ok_or_else(|| {
                            fallout_data::Error::Resolution("unterminated MAST".into())
                        })?;
                        let master = std::str::from_utf8(bytes).map_err(|_| {
                            fallout_data::Error::Resolution("unsupported MAST encoding".into())
                        })?;
                        let normalized = plugin_name(master)?;
                        if !master_names.insert(normalized) {
                            return Err(fallout_data::Error::Resolution("duplicate MAST".into()));
                        }
                        report.masters.push(master.into());
                    }
                }
                if sub.kind != *b"VMAD" {
                    return Ok(());
                }
                record_vmad_count += 1;
                if record_vmad_count > 1 {
                    report.issue(format!("duplicate VMAD at record 0x{:X}", header.offset));
                }
                report.vmad_records += 1;
                if sub.data.len() >= 4 {
                    *report
                        .vmad_versions
                        .entry(u16::from_le_bytes([sub.data[0], sub.data[1]]))
                        .or_default() += 1;
                    *report
                        .vmad_object_formats
                        .entry(u16::from_le_bytes([sub.data[2], sub.data[3]]))
                        .or_default() += 1;
                }
                match vmad::decode_record(sub.data, header.kind, &name, vmad::Limits::default()) {
                    Ok(attachment) => {
                        observe(Observation::Binding {
                            record,
                            subrecord_offset: sub.payload_offset,
                            attachment: &attachment,
                        })?;
                        if !attachment.primary.undecoded_tail.is_empty() {
                            report.vmad_tails += 1;
                        }
                        for script in attachment.primary.scripts {
                            report.vmad_properties += script.properties.len() as u64;
                            report.properties(&script, header);
                            track_script(
                                &mut scripts,
                                script.name,
                                UseKind::Primary(script.status),
                                header,
                            )?;
                        }
                        match attachment.tail {
                            vmad::Tail::Absent => {}
                            vmad::Tail::Unsupported { reason, .. } => {
                                report.vmad_tail_failures += 1;
                                report.issue(format!(
                                    "record {:08X} at 0x{:X}: {reason}",
                                    header.form_id, header.offset
                                ));
                            }
                            vmad::Tail::Decoded(extension) => {
                                report.vmad_decoded_tails += 1;
                                if !extension.file_name.is_empty() {
                                    track_script(
                                        &mut scripts,
                                        extension.file_name,
                                        UseKind::FragmentFile,
                                        header,
                                    )?;
                                }
                                for fragment in extension.fragments {
                                    let kind = match fragment.selector {
                                        vmad::Selector::FlagBit(_) => {
                                            plugin::signature(header.kind)
                                        }
                                        vmad::Selector::PerkIndex(_) => "PERK".into(),
                                        vmad::Selector::Quest { .. } => "QUST".into(),
                                        vmad::Selector::ScenePhase { .. } => "SCEN-phase".into(),
                                    };
                                    *report.fragment_kinds.entry(kind).or_default() += 1;
                                    track_script(
                                        &mut scripts,
                                        fragment.script_name,
                                        UseKind::Fragment,
                                        header,
                                    )?;
                                }
                                report.quest_aliases += extension.aliases.len() as u64;
                                for alias in extension.aliases {
                                    report.object(&alias.object, header, alias.offset);
                                    for script in alias.scripts {
                                        report.alias_properties += script.properties.len() as u64;
                                        report.properties(&script, header);
                                        track_script(
                                            &mut scripts,
                                            script.name,
                                            UseKind::Alias(script.status),
                                            header,
                                        )?;
                                    }
                                }
                            }
                        }
                    }
                    Err(error) => {
                        report.vmad_prefix_failures += 1;
                        report.issue(format!(
                            "record {:08X} at 0x{:X}, VMAD payload +0x{:X}: {error}",
                            header.form_id, header.offset, sub.payload_offset
                        ));
                    }
                }
                Ok(())
            })?;
            Ok(())
        },
    )?;
    if !hedr_seen {
        return Err(bad(&name, 0, "TES4 has no HEDR"));
    }
    report.scripts = scripts.into_values().collect();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn anniversary_extended_esl_range_does_not_truncate_full_ids() {
        assert!(!valid_light_local_id(1.7, 0x7FF));
        assert!(valid_light_local_id(1.7, 0x800));
        assert!(valid_light_local_id(1.71, 0x001));
        assert!(valid_light_local_id(1.71, 0xFFF));
        assert!(!valid_light_local_id(1.71, 0x1000));
        assert!(!valid_light_local_id(1.71, 0xFE001800));
        assert!(!valid_light_local_id(f32::NAN, 0x800));
        assert!(!valid_light_local_id(2.0, 0x800));
    }
}
