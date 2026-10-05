//! Skyrim-owned full/light runtime-slot mapping over an explicitly ordered
//! active-plugin list. This is separate from Fallout's shared profile identity.
use crate::{Error, Result, plugin};
use fallout_data::identity::plugin_name;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const FULL_PLUGIN_LIMIT: usize = 0xFE;
const LIGHT_PLUGIN_LIMIT: usize = 0x1000;
const ORDER_INPUT_LIMIT: usize = 1024 * 1024;

/// Input to `map-profile`. The sequence must contain active plugins in the
/// exact order supplied by the caller. This parser does not infer order from
/// installed files or from a Creation filename.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExplicitOrder {
    pub schema_version: u32,
    pub active_plugins: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrderFingerprint {
    pub file_name: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema_version: u32,
    pub order_input: OrderFingerprint,
    pub mapping: RuntimeProfile,
    pub plugin_findings: Vec<PluginFindings>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginFindings {
    pub file: String,
    pub count: u64,
    pub examples: Vec<String>,
}

pub fn decode_explicit_order(bytes: &[u8]) -> Result<ExplicitOrder> {
    if bytes.len() > ORDER_INPUT_LIMIT {
        return Err(Error::Unsupported(
            "explicit active-order JSON exceeds 1 MiB".into(),
        ));
    }
    let order: ExplicitOrder = serde_json::from_slice(bytes)?;
    if order.schema_version != 1 {
        return Err(Error::Unsupported(format!(
            "unsupported explicit active-order schema {}",
            order.schema_version
        )));
    }
    if order.active_plugins.len() > FULL_PLUGIN_LIMIT + LIGHT_PLUGIN_LIMIT {
        return Err(Error::Unsupported(
            "explicit active-order entry budget exceeded".into(),
        ));
    }
    let mut names = std::collections::BTreeSet::new();
    for file in &order.active_plugins {
        if !names.insert(plugin_name(file)?) {
            return Err(Error::Unsupported(format!(
                "duplicate active plugin {file:?}"
            )));
        }
    }
    Ok(order)
}

#[derive(Debug, Clone, Serialize)]
pub struct MappedPlugin {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    pub light: bool,
    pub esl_extension: bool,
    pub header_version_bits: u32,
    pub masters: Vec<String>,
    /// Position among active non-light plugins; absent for light plugins.
    pub full_slot: Option<u8>,
    /// Position among active light plugins; absent for full plugins.
    pub light_slot: Option<u16>,
}

/// Resolved indices are candidates based on an explicit active order. They do
/// not certify that the retail executable consumed this order or that a FormID
/// names a record which exists.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProfile {
    pub schema_version: u32,
    pub status: &'static str,
    pub active_order: Vec<String>,
    pub plugins: Vec<MappedPlugin>,
    pub limitations: Vec<&'static str>,
    #[serde(skip)]
    full_by_name: BTreeMap<String, usize>,
    #[serde(skip)]
    light_by_name: BTreeMap<String, usize>,
    #[serde(skip)]
    full_by_slot: Vec<String>,
    #[serde(skip)]
    light_by_slot: Vec<String>,
    #[serde(skip)]
    header_version_by_name: BTreeMap<String, u32>,
}

impl RuntimeProfile {
    pub fn build(installed: &[plugin::PluginReport], active_order: &[String]) -> Result<Self> {
        if active_order.is_empty() {
            return Err(Error::Unsupported("active plugin order is empty".into()));
        }
        let mut available = BTreeMap::new();
        for item in installed {
            let key = plugin_name(&item.file)?;
            if available.insert(key, item).is_some() {
                return Err(Error::Unsupported(
                    "installed plugin names collide case-insensitively".into(),
                ));
            }
        }

        let mut ordered = Vec::with_capacity(active_order.len());
        let mut positions = BTreeMap::new();
        for (position, file) in active_order.iter().enumerate() {
            let key = plugin_name(file)?;
            if positions.insert(key.clone(), position).is_some() {
                return Err(Error::Unsupported(format!(
                    "duplicate active plugin {file:?}"
                )));
            }
            let report = available.get(&key).ok_or_else(|| {
                Error::Unsupported(format!(
                    "active plugin {file:?} is absent from the inspected set"
                ))
            })?;
            if !matches!(
                report.header_version_bits,
                value if value == 1.7f32.to_bits() || value == 1.71f32.to_bits()
            ) {
                return Err(Error::Unsupported(format!(
                    "{} has an unverified Skyrim HEDR version",
                    report.file
                )));
            }
            ordered.push((key, *report));
        }

        for (position, (key, report)) in ordered.iter().enumerate() {
            for master in &report.masters {
                let master_key = plugin_name(master)?;
                let Some(master_position) = positions.get(&master_key) else {
                    return Err(Error::Unsupported(format!(
                        "active plugin {} requires inactive or absent master {master}",
                        report.file
                    )));
                };
                if *master_position >= position {
                    return Err(Error::Unsupported(format!(
                        "master {master} must precede dependent plugin {}",
                        report.file
                    )));
                }
            }
            if key != &plugin_name(&report.file)? {
                return Err(Error::Unsupported(
                    "plugin identity normalization changed".into(),
                ));
            }
        }

        let full_count = ordered.iter().filter(|(_, report)| !report.light).count();
        let light_count = ordered.iter().filter(|(_, report)| report.light).count();
        if full_count > FULL_PLUGIN_LIMIT {
            return Err(Error::Unsupported(format!(
                "{} active full plugins exceed the Skyrim 0x00-0xFD slot range",
                full_count
            )));
        }
        if light_count > LIGHT_PLUGIN_LIMIT {
            return Err(Error::Unsupported(format!(
                "{} active light plugins exceed the Skyrim 0x000-0xFFF slot range",
                light_count
            )));
        }

        let mut next_full = 0usize;
        let mut next_light = 0usize;
        let mut mapped = Vec::with_capacity(ordered.len());
        let mut full_by_name = BTreeMap::new();
        let mut light_by_name = BTreeMap::new();
        let mut full_by_slot = Vec::new();
        let mut light_by_slot = Vec::new();
        let mut header_version_by_name = BTreeMap::new();
        for (key, report) in ordered {
            header_version_by_name.insert(key.clone(), report.header_version_bits);
            let (full_slot, light_slot) = if report.light {
                let slot = next_light;
                next_light += 1;
                light_by_name.insert(key.clone(), slot);
                light_by_slot.push(key);
                (None, Some(slot as u16))
            } else {
                let slot = next_full;
                next_full += 1;
                full_by_name.insert(key.clone(), slot);
                full_by_slot.push(key);
                (Some(slot as u8), None)
            };
            mapped.push(MappedPlugin {
                file: report.file.clone(),
                bytes: report.bytes,
                sha256: report.sha256.clone(),
                light: report.light,
                esl_extension: report.esl_extension,
                header_version_bits: report.header_version_bits,
                masters: report.masters.clone(),
                full_slot,
                light_slot,
            });
        }
        Ok(Self {
            schema_version: 1,
            status: "explicit-order mapping computed; live game order and winning overrides not certified",
            active_order: active_order.to_vec(),
            plugins: mapped,
            limitations: vec![
                "Active order is caller supplied; installed-file presence and Creation filenames do not establish activation",
                "The executable's consumed profile and active order are not observed by this mapper",
                "Resolved identities are slot candidates; record existence, override winners, and runtime behavior are not tested",
                "Cross-file on-disk FE encodings are not interpreted by the runtime-ID decoder",
            ],
            full_by_name,
            light_by_name,
            full_by_slot,
            light_by_slot,
            header_version_by_name,
        })
    }

    /// Encode an origin/local identity into Skyrim's runtime FormID space.
    ///
    /// This does not rewrite plugin bytes and does not imply the target record
    /// exists. It only uses the slots in the explicitly supplied active order.
    pub fn runtime_form_id(&self, source: &crate::trace::SourceKey) -> Result<u32> {
        let key = plugin_name(&source.origin_plugin)?;
        if let Some(slot) = self.light_by_name.get(&key) {
            let header_version = self
                .header_version_by_name
                .get(&key)
                .ok_or_else(|| Error::Unsupported("light slot has no plugin metadata".into()))?;
            if !plugin::valid_light_local_id(f32::from_bits(*header_version), source.local_id) {
                return Err(Error::Unsupported(format!(
                    "local ID {:06X} is outside {}'s declared light range",
                    source.local_id, source.origin_plugin
                )));
            }
            return Ok(0xFE00_0000 | ((*slot as u32) << 12) | source.local_id);
        }
        let Some(slot) = self.full_by_name.get(&key) else {
            return Err(Error::Unsupported(format!(
                "source plugin {:?} is not active in this order",
                source.origin_plugin
            )));
        };
        if source.local_id > 0x00FF_FFFF {
            return Err(Error::Unsupported(
                "full-plugin local ID exceeds 24 bits".into(),
            ));
        }
        Ok(((*slot as u32) << 24) | source.local_id)
    }

    /// Resolve an already runtime-encoded FormID into a physical plugin/local
    /// identity. Zero and runtime-reserved/unassigned slots remain unresolved.
    pub fn source_from_runtime_form_id(&self, raw: u32) -> Result<Option<crate::trace::SourceKey>> {
        if raw == 0 {
            return Ok(None);
        }
        let selector = raw >> 24;
        if selector == 0xFE {
            let slot = ((raw >> 12) & 0x0FFF) as usize;
            let Some(name) = self.light_by_slot.get(slot) else {
                return Ok(None);
            };
            let header_version = self
                .header_version_by_name
                .get(name)
                .ok_or_else(|| Error::Unsupported("light slot has no plugin metadata".into()))?;
            let local_id = raw & 0x0FFF;
            if !plugin::valid_light_local_id(f32::from_bits(*header_version), local_id) {
                return Ok(None);
            }
            return Ok(Some(crate::trace::SourceKey {
                origin_plugin: name.clone(),
                local_id,
            }));
        }
        if selector >= 0xFE {
            return Ok(None);
        }
        let Some(name) = self.full_by_slot.get(selector as usize) else {
            return Ok(None);
        };
        Ok(Some(crate::trace::SourceKey {
            origin_plugin: name.clone(),
            local_id: raw & 0x00FF_FFFF,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn metadata(file: &str, light: bool, version: f32, masters: &[&str]) -> plugin::PluginReport {
        plugin::PluginReport {
            file: file.into(),
            bytes: 100,
            sha256: "00".repeat(32),
            header_version_bits: version.to_bits(),
            header_flags: if light { 0x200 } else { 0 },
            light,
            esl_extension: file.to_ascii_lowercase().ends_with(".esl"),
            localized: false,
            masters: masters.iter().map(|name| (*name).into()).collect(),
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
            status: "synthetic metadata",
        }
    }

    fn profile() -> RuntimeProfile {
        let installed = vec![
            metadata("Skyrim.esm", false, 1.7, &[]),
            metadata("Light.esp", true, 1.7, &["Skyrim.esm"]),
            metadata("Patch.esl", true, 1.71, &["Skyrim.esm", "Light.esp"]),
            metadata("Addon.esp", false, 1.7, &["Skyrim.esm", "Patch.esl"]),
        ];
        RuntimeProfile::build(
            &installed,
            &[
                "Skyrim.esm".into(),
                "Light.esp".into(),
                "Patch.esl".into(),
                "Addon.esp".into(),
            ],
        )
        .unwrap()
    }

    #[test]
    fn full_and_light_slots_have_independent_runtime_spaces() {
        let profile = profile();
        assert_eq!(profile.plugins[0].full_slot, Some(0));
        assert_eq!(profile.plugins[1].light_slot, Some(0));
        assert_eq!(profile.plugins[2].light_slot, Some(1));
        assert_eq!(profile.plugins[3].full_slot, Some(1));
        let full = crate::trace::SourceKey {
            origin_plugin: "addon.esp".into(),
            local_id: 0x123456,
        };
        let light = crate::trace::SourceKey {
            origin_plugin: "patch.esl".into(),
            local_id: 0x001,
        };
        assert_eq!(profile.runtime_form_id(&full).unwrap(), 0x01123456);
        assert_eq!(profile.runtime_form_id(&light).unwrap(), 0xFE001001);
        assert_eq!(
            profile.source_from_runtime_form_id(0xFE001001).unwrap(),
            Some(light)
        );
        assert_eq!(
            profile.source_from_runtime_form_id(0x01123456).unwrap(),
            Some(full)
        );
        assert_eq!(
            profile.source_from_runtime_form_id(0xFF000001).unwrap(),
            None
        );
    }

    #[test]
    fn light_slot_status_comes_from_header_flag_not_esl_suffix() {
        let installed = vec![
            metadata("Base.esm", false, 1.7, &[]),
            metadata("SuffixOnly.esl", false, 1.7, &["Base.esm"]),
        ];
        let profile =
            RuntimeProfile::build(&installed, &["Base.esm".into(), "SuffixOnly.esl".into()])
                .unwrap();
        assert_eq!(profile.plugins[1].full_slot, Some(1));
        assert_eq!(profile.plugins[1].light_slot, None);
    }

    #[test]
    fn active_order_rejects_missing_masters_duplicates_and_bad_light_ids() {
        let installed = vec![
            metadata("Base.esm", false, 1.7, &[]),
            metadata("Child.esp", false, 1.7, &["Base.esm"]),
            metadata("Light.esl", true, 1.7, &["Base.esm"]),
        ];
        assert!(RuntimeProfile::build(&installed, &["Child.esp".into()]).is_err());
        assert!(
            RuntimeProfile::build(&installed, &["Child.esp".into(), "Base.esm".into()]).is_err()
        );
        assert!(
            RuntimeProfile::build(&installed, &["Base.esm".into(), "base.ESM".into()]).is_err()
        );
        let profile =
            RuntimeProfile::build(&installed, &["Base.esm".into(), "Light.esl".into()]).unwrap();
        assert!(
            profile
                .runtime_form_id(&crate::trace::SourceKey {
                    origin_plugin: "Light.esl".into(),
                    local_id: 0x7FF,
                })
                .is_err()
        );
    }

    #[test]
    fn explicit_order_json_is_bounded_versioned_and_strict() {
        let parsed =
            decode_explicit_order(br#"{"schema_version":1,"active_plugins":["Skyrim.esm"]}"#)
                .unwrap();
        assert_eq!(parsed.active_plugins, ["Skyrim.esm"]);
        assert!(
            decode_explicit_order(br#"{"schema_version":2,"active_plugins":["Skyrim.esm"]}"#)
                .is_err()
        );
        assert!(
            decode_explicit_order(
                br#"{"schema_version":1,"active_plugins":[],"guess_from_install":true}"#
            )
            .is_err()
        );
        assert!(decode_explicit_order(&vec![b' '; 1024 * 1024 + 1]).is_err());
    }
}
