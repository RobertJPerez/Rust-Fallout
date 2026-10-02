use crate::{Error, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileId {
    NvOriginal,
    Fo3Original,
    TtwCompatible,
    Fo4Original,
    Fo76Research,
    StarfieldProbe,
    UnifiedCrossover,
}

/// A definition's identity is independent of where its plugin sits in a load order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct FormKey {
    pub profile: ProfileId,
    pub origin_plugin: String,
    pub local_id: u32,
}

pub fn plugin_name(name: &str) -> Result<String> {
    if name.is_empty()
        || !name.is_ascii()
        || name.bytes().any(|b| b < 32 || b"/\\:".contains(&b))
        || matches!(name, "." | "..")
    {
        return Err(Error::Unsupported(format!(
            "plugin name or encoding {name:?}"
        )));
    }
    Ok(name.to_ascii_lowercase())
}

/// On disk, the high byte selects a master. Any selector beyond the master list
/// names the current plugin (xEdit FileFileIDtoLoadOrderFileID and esplugin).
/// Gun Runners' Arsenal contains real examples with a noncanonical self selector.
/// It is not the plugin's position in the final runtime load order.
pub fn resolve_form(
    profile: ProfileId,
    plugin: &str,
    masters: &[String],
    id: u32,
) -> Result<Option<FormKey>> {
    if id == 0 {
        return Ok(None);
    }
    let slot = (id >> 24) as usize;
    let origin = if slot >= masters.len() {
        plugin
    } else {
        &masters[slot]
    };
    Ok(Some(FormKey {
        profile,
        origin_plugin: plugin_name(origin)?,
        local_id: id & 0x00ff_ffff,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_survives_rebasing_and_keeps_campaigns_separate() {
        let original = resolve_form(ProfileId::NvOriginal, "Base.esm", &[], 0x123).unwrap();
        let override_key = resolve_form(
            ProfileId::NvOriginal,
            "Patch.esp",
            &["Base.esm".into()],
            0x123,
        )
        .unwrap();
        assert_eq!(original, override_key);
        assert_ne!(
            original,
            resolve_form(ProfileId::Fo3Original, "Base.esm", &[], 0x123).unwrap()
        );
        assert_ne!(
            original,
            resolve_form(ProfileId::NvOriginal, "Other.esm", &[], 0x123).unwrap()
        );
        assert_eq!(
            resolve_form(
                ProfileId::NvOriginal,
                "Patch.esp",
                &["Base.esm".into()],
                0x01000801
            )
            .unwrap(),
            resolve_form(
                ProfileId::NvOriginal,
                "Patch.esp",
                &["Base.esm".into()],
                0x02000801
            )
            .unwrap()
        );
    }
}
