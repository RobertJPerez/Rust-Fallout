use crate::{
    Error, Result,
    baseline::open_source,
    identity::{FormKey, ProfileId, plugin_name, resolve_form},
    io, malformed,
    plugin::{self, Event, Limits, RecordHeader, Subrecord},
    script_inventory::{ScriptInventory, ScriptReference},
};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::BufReader,
    path::Path,
};

#[derive(Debug, Clone, Default, Serialize)]
pub struct Count {
    pub occurrences: u64,
    pub decoded_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct PluginCensus {
    pub scripts: ScriptInventory,
    pub integrity_issues: Vec<plugin::ChecksumMismatch>,
    pub name: String,
    pub source_bytes: u64,
    pub header_version: f32,
    pub declared_records_and_groups: u32,
    pub records_excluding_header: u64,
    pub groups: u64,
    pub masters: Vec<String>,
    pub compressed_records: u64,
    pub deleted_records: u64,
    pub persistent_records: u64,
    pub initially_disabled_records: u64,
    pub record_kinds: BTreeMap<String, Count>,
    pub subrecord_kinds: BTreeMap<String, Count>,
    pub form_versions: BTreeMap<u16, u64>,
    pub group_kinds: BTreeMap<i32, u64>,
    pub status: &'static str,
    pub unknown: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Definition {
    pub header: RecordHeader,
    pub editor_id: Option<Vec<u8>>,
    pub parent: ParentContext,
}

/// Raw IDs belong to this definition's source plugin, including group labels.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ParentContext {
    pub world: Option<u32>,
    pub cell: Option<u32>,
    pub child_group: Option<i32>,
}

pub struct PluginIndex {
    pub census: PluginCensus,
    pub records: Vec<Definition>,
    pub script_references: Vec<ScriptReference>,
}

fn terminated_string<'a>(sub: &'a Subrecord<'_>, source: &str, offset: u64) -> Result<&'a [u8]> {
    let raw = sub
        .data
        .strip_suffix(&[0])
        .ok_or_else(|| malformed(source, offset, "string lacks NUL terminator"))?;
    if raw.contains(&0) {
        return Err(malformed(source, offset, "string contains embedded NUL"));
    }
    Ok(raw)
}

pub fn index_plugin(path: &Path) -> Result<PluginIndex> {
    index_plugin_with_limits(path, Limits::default())
}

pub fn index_plugin_with_limits(path: &Path, limits: Limits) -> Result<PluginIndex> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| Error::Unsupported("plugin filename encoding".into()))?
        .to_owned();
    plugin_name(&name)?;
    let file = open_source(path)?;
    let length = file.metadata().map_err(|e| io(path, e))?.len();
    let mut census = PluginCensus {
        scripts: ScriptInventory::default(),
        integrity_issues: vec![],
        name: name.clone(),
        source_bytes: length,
        header_version: 0.0,
        declared_records_and_groups: 0,
        records_excluding_header: 0,
        groups: 0,
        masters: vec![],
        compressed_records: 0,
        deleted_records: 0,
        persistent_records: 0,
        initially_disabled_records: 0,
        record_kinds: BTreeMap::new(),
        subrecord_kinds: BTreeMap::new(),
        form_versions: BTreeMap::new(),
        group_kinds: BTreeMap::new(),
        status: "framing-decoded; gameplay semantics unimplemented",
        unknown: vec![
            "typed references within fields",
            "script opcodes and native calls",
            "condition evaluation",
            "record-specific override exceptions",
            "asset dependency resolution",
        ],
    };
    let mut records = Vec::new();
    let mut script_references = Vec::new();
    let mut hedr_seen = false;
    let mut previous_master = false;
    let mut master_set = BTreeSet::new();
    let mut ids = BTreeSet::new();
    let mut groups: Vec<plugin::Group> = Vec::new();
    plugin::visit(&mut BufReader::new(file), length, &name, limits, |event| {
        let offset = match &event {
            Event::Group(g) => g.offset,
            Event::Record(r) => r.header.offset,
        };
        while groups
            .last()
            .is_some_and(|g| offset >= g.offset + u64::from(g.size))
        {
            groups.pop();
        }
        let record = match event {
            Event::Group(g) => {
                census.groups += 1;
                *census.group_kinds.entry(g.kind).or_default() += 1;
                groups.push(g.clone());
                return Ok(());
            }
            Event::Record(r) => r,
        };
        let h = &record.header;
        if let Some(issue) = &record.integrity_issue {
            census.integrity_issues.push(issue.clone());
        }
        let header = h.offset == 0;
        if !header {
            if !hedr_seen {
                return Err(malformed(&name, h.offset, "missing HEDR"));
            }
            if h.form_id == 0 {
                return Err(malformed(
                    &name,
                    h.offset,
                    "non-header record has null FormID",
                ));
            }
            if !ids.insert(h.form_id) {
                return Err(malformed(
                    &name,
                    h.offset,
                    format!("duplicate FormID {:08X}", h.form_id),
                ));
            }
            census.records_excluding_header += 1;
        } else if h.form_id != 0 {
            return Err(malformed(&name, 0, "TES4 header FormID must be zero"));
        }
        let kind = plugin::signature(h.kind);
        let count = census.record_kinds.entry(kind.clone()).or_default();
        count.occurrences += 1;
        count.decoded_bytes += record.payload.len() as u64;
        *census.form_versions.entry(h.version).or_default() += 1;
        census.compressed_records += u64::from(h.flags & plugin::COMPRESSED != 0);
        census.deleted_records += u64::from(h.flags & plugin::DELETED != 0);
        census.persistent_records += u64::from(h.flags & plugin::PERSISTENT != 0);
        census.initially_disabled_records += u64::from(h.flags & plugin::INITIALLY_DISABLED != 0);
        let mut editor_id = None;
        plugin::visit_subrecords(record, &name, |sub| {
            if let Some(reference) = census.scripts.observe(h, &sub, &name)? {
                script_references.push(reference);
            }
            let count = census
                .subrecord_kinds
                .entry(format!("{kind}/{}", plugin::signature(sub.kind)))
                .or_default();
            count.occurrences += 1;
            count.decoded_bytes += sub.data.len() as u64;
            if header {
                if previous_master && sub.kind != *b"DATA" {
                    return Err(malformed(&name, 0, "MAST lacks its DATA field"));
                }
                match &sub.kind {
                    b"HEDR" => {
                        if hedr_seen || sub.data.len() != 12 {
                            return Err(malformed(&name, 0, "invalid HEDR"));
                        }
                        census.header_version =
                            f32::from_le_bytes(sub.data[..4].try_into().expect("checked length"));
                        // These versions occur in the supplied official NV corpus. They share
                        // this framing; individual fields still need versioned decoders.
                        if ![1.32f32.to_bits(), 1.33f32.to_bits(), 1.34f32.to_bits()]
                            .contains(&census.header_version.to_bits())
                        {
                            return Err(Error::Unsupported(format!(
                                "{name}: FNV HEDR version {}",
                                census.header_version
                            )));
                        }
                        census.declared_records_and_groups =
                            u32::from_le_bytes(sub.data[4..8].try_into().expect("checked length"));
                        hedr_seen = true;
                    }
                    b"MAST" => {
                        let raw = terminated_string(&sub, &name, 0)?;
                        let master = std::str::from_utf8(raw).map_err(|_| {
                            Error::Unsupported(format!("{name}: master filename encoding"))
                        })?;
                        let key = plugin_name(master)?;
                        if !master_set.insert(key) {
                            return Err(malformed(&name, 0, "duplicate master"));
                        }
                        if census.masters.len() >= 254 {
                            return Err(malformed(&name, 0, "too many masters"));
                        }
                        census.masters.push(master.to_owned());
                        previous_master = true;
                    }
                    b"DATA" if previous_master => {
                        if sub.data.len() != 8 {
                            return Err(malformed(&name, 0, "master DATA must be eight bytes"));
                        }
                        previous_master = false;
                    }
                    _ => {}
                }
            }
            if sub.kind == *b"EDID" {
                editor_id = Some(terminated_string(&sub, &name, h.offset)?.to_vec());
            }
            Ok(())
        })?;
        if header && (!hedr_seen || previous_master) {
            return Err(malformed(&name, 0, "incomplete plugin header"));
        }
        if !header {
            let mut parent = ParentContext::default();
            for group in &groups {
                let label = u32::from_le_bytes(group.label);
                match group.kind {
                    1 => parent.world = Some(label),
                    6 => parent.cell = Some(label),
                    8..=10 => {
                        if parent.cell != Some(label) {
                            return Err(malformed(
                                &name,
                                group.offset,
                                "cell child group disagrees with its parent label",
                            ));
                        }
                        parent.child_group = Some(group.kind);
                    }
                    _ => {}
                }
            }
            records.push(Definition {
                header: h.clone(),
                editor_id,
                parent,
            });
        }
        Ok(())
    })?;
    if !census.integrity_issues.is_empty() {
        census.status = "UNTRUSTED diagnostic decode; strict integrity check failed";
    }
    Ok(PluginIndex {
        census,
        records,
        script_references,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct Origin {
    pub plugin: String,
    pub file_offset: u64,
    pub flags: u32,
    pub kind: [u8; 4],
}

pub struct ResolvedContent {
    pub profile: ProfileId,
    pub chains: BTreeMap<FormKey, Vec<Origin>>,
}

#[derive(Debug, Serialize)]
pub struct ResolutionReport {
    pub integrity_failures: usize,
    pub script_links: LinkReport,
    pub explicit_load_order: Vec<String>,
    pub unique_definitions: usize,
    pub overridden_definitions: usize,
    pub overrides: Vec<OverrideChain>,
    pub semantic_status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct OverrideChain {
    pub key: FormKey,
    pub sources: Vec<Origin>,
}

/// Resolve definitions only. A whole-record winner here is a structural fact;
/// record-specific runtime merge behavior still needs its own schema and tests.
pub fn resolve(indices: &[PluginIndex], profile: ProfileId) -> Result<ResolvedContent> {
    resolve_structure(indices, profile, false)
}

/// Diagnostic tooling can inspect links around a damaged record, but the report
/// carries every integrity failure and cannot serve as a runtime acceptance result.
pub fn inspect_resolution(indices: &[PluginIndex], profile: ProfileId) -> Result<ResolutionReport> {
    Ok(resolve_structure(indices, profile, true)?.report(indices))
}

pub(crate) fn resolve_structure(
    indices: &[PluginIndex],
    profile: ProfileId,
    diagnostic: bool,
) -> Result<ResolvedContent> {
    let mut loaded = BTreeSet::new();
    let mut chains: BTreeMap<FormKey, Vec<Origin>> = BTreeMap::new();
    for index in indices {
        if !diagnostic && !index.census.integrity_issues.is_empty() {
            return Err(Error::Resolution(format!(
                "{} has integrity failures and cannot supply resolved runtime content",
                index.census.name
            )));
        }
        let name = &index.census.name;
        let key = plugin_name(name)?;
        if loaded.contains(&key) {
            return Err(Error::Resolution(format!("duplicate plugin {name}")));
        }
        for master in &index.census.masters {
            if !loaded.contains(&plugin_name(master)?) {
                return Err(Error::Resolution(format!(
                    "{name}: master {master} is missing, cyclic, or occurs later in the supplied order"
                )));
            }
        }
        let mut own_keys = BTreeSet::new();
        for record in &index.records {
            let form = resolve_form(profile, name, &index.census.masters, record.header.form_id)?
                .ok_or_else(|| Error::Resolution(format!("{name}: null definition")))?;
            if !own_keys.insert(form.clone()) {
                return Err(Error::Resolution(format!(
                    "{name}: multiple raw IDs alias the same origin/local identity"
                )));
            }
            chains.entry(form).or_default().push(Origin {
                plugin: name.clone(),
                file_offset: record.header.offset,
                flags: record.header.flags,
                kind: record.header.kind,
            });
        }
        loaded.insert(key);
    }
    Ok(ResolvedContent { profile, chains })
}

impl ResolvedContent {
    pub fn report(&self, indices: &[PluginIndex]) -> ResolutionReport {
        let overrides: Vec<_> = self
            .chains
            .iter()
            .filter(|(_, c)| c.len() > 1)
            .map(|(key, sources)| OverrideChain {
                key: key.clone(),
                sources: sources.clone(),
            })
            .collect();
        ResolutionReport {
            integrity_failures: indices
                .iter()
                .map(|i| i.census.integrity_issues.len())
                .sum(),
            script_links: self.link_scripts(indices),
            explicit_load_order: indices.iter().map(|i| i.census.name.clone()).collect(),
            unique_definitions: self.chains.len(),
            overridden_definitions: overrides.len(),
            overrides,
            semantic_status: "structural override chains and SCRO links only; all other field links and merge exceptions unverified",
        }
    }

    fn link_scripts(&self, indices: &[PluginIndex]) -> LinkReport {
        let mut report = LinkReport::default();
        // All symbols already exist. Forward links and legitimate record cycles do
        // not need recursive loading, and cannot accidentally create duplicate forms.
        for index in indices {
            for reference in &index.script_references {
                report.occurrences += 1;
                let resolved = resolve_form(
                    self.profile,
                    &index.census.name,
                    &index.census.masters,
                    reference.target_raw_form,
                );
                let reason = match resolved {
                    Ok(None) => {
                        report.null_references += 1;
                        continue;
                    }
                    Ok(Some(key)) if self.chains.contains_key(&key) => {
                        report.resolved += 1;
                        continue;
                    }
                    Ok(Some(key))
                        if key.profile == ProfileId::NvOriginal
                            && key.origin_plugin == "falloutnv.esm"
                            && key.local_id == 0x14 =>
                    {
                        // xEdit supplies PlayerRef from its hardcoded module. It is
                        // an engine binding, not a missing record to invent in the ESM.
                        // Recognizing that dependency does not implement a player.
                        let binding = report
                            .runtime_dependencies
                            .entry(RuntimeBinding::NvPlayerReference)
                            .or_default();
                        binding.occurrences += 1;
                        if binding.examples.len() < 3 {
                            binding.examples.push(LinkOrigin {
                                plugin: index.census.name.clone(),
                                reference: reference.clone(),
                            });
                        }
                        continue;
                    }
                    Ok(Some(key)) => format!(
                        "missing definition {}:{:06X}",
                        key.origin_plugin, key.local_id
                    ),
                    Err(error) => error.to_string(),
                };
                report.unresolved.push(UnresolvedLink {
                    plugin: index.census.name.clone(),
                    reference: reference.clone(),
                    reason,
                });
            }
        }
        report
    }
}

#[derive(Debug, Default, Serialize)]
pub struct LinkReport {
    pub occurrences: u64,
    pub resolved: u64,
    pub null_references: u64,
    pub runtime_dependencies: BTreeMap<RuntimeBinding, RuntimeDependency>,
    pub unresolved: Vec<UnresolvedLink>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeBinding {
    NvPlayerReference,
}

#[derive(Debug, Default, Serialize)]
pub struct RuntimeDependency {
    pub occurrences: u64,
    pub examples: Vec<LinkOrigin>,
}

#[derive(Debug, Serialize)]
pub struct LinkOrigin {
    pub plugin: String,
    pub reference: ScriptReference,
}

#[derive(Debug, Serialize)]
pub struct UnresolvedLink {
    pub plugin: String,
    pub reference: ScriptReference,
    pub reason: String,
}
