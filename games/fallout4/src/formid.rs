//! Raw Fallout 4 FormID bit observations; deliberately does not assign identities.
use serde::Serialize;

pub const FALLOUT4_SMALL_MASTER_FLAG: u32 = 0x0000_0200;
pub const SMALL_MASTER_MARKER: u8 = 0xFE;
pub const MEDIUM_MASTER_MARKER: u8 = 0xFD;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RawFormIdPattern {
    ZeroValue,
    FullWidthPattern,
    SmallMarkerPattern,
    MediumMarkerPattern,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct RawFormIdObservation {
    /// Exact little-endian value from the plugin record header.
    pub raw: u32,
    pub pattern: RawFormIdPattern,
    pub raw_high_byte: u8,
    pub full_low_24_candidate: u32,
    /// Present only when the raw high byte is 0xFE; this is not a load-order slot.
    pub small_selector_candidate: Option<u16>,
    pub small_low_12_candidate: Option<u16>,
    /// Present only when the raw high byte is 0xFD; Fallout 4 support is unasserted.
    pub medium_selector_candidate: Option<u8>,
    pub medium_low_16_candidate: Option<u16>,
    /// A raw full-width selector outside the supplied plugin master list is kept
    /// unresolved. It may denote the current plugin under a dialect-specific rule.
    pub full_selector_outside_master_list: Option<bool>,
}

pub fn observe(raw: u32, listed_masters: usize) -> RawFormIdObservation {
    let selector = (raw >> 24) as u8;
    let pattern = match (raw, selector) {
        (0, _) => RawFormIdPattern::ZeroValue,
        (_, SMALL_MASTER_MARKER) => RawFormIdPattern::SmallMarkerPattern,
        (_, MEDIUM_MASTER_MARKER) => RawFormIdPattern::MediumMarkerPattern,
        _ => RawFormIdPattern::FullWidthPattern,
    };
    let small = (pattern == RawFormIdPattern::SmallMarkerPattern).then_some({
        (
            ((raw & 0x00FF_F000) >> 12) as u16,
            (raw & 0x0000_0FFF) as u16,
        )
    });
    let medium = (pattern == RawFormIdPattern::MediumMarkerPattern).then_some({
        (
            ((raw & 0x00FF_0000) >> 16) as u8,
            (raw & 0x0000_FFFF) as u16,
        )
    });
    let full_selector_outside_master_list = (pattern == RawFormIdPattern::FullWidthPattern)
        .then(|| usize::from(selector) >= listed_masters);

    RawFormIdObservation {
        raw,
        pattern,
        raw_high_byte: selector,
        full_low_24_candidate: raw & 0x00FF_FFFF,
        small_selector_candidate: small.map(|(index, _)| index),
        small_low_12_candidate: small.map(|(_, local)| local),
        medium_selector_candidate: medium.map(|(index, _)| index),
        medium_low_16_candidate: medium.map(|(_, local)| local),
        full_selector_outside_master_list,
    }
}

pub fn is_small_master_flag(header_flags: u32) -> bool {
    header_flags & FALLOUT4_SMALL_MASTER_FLAG != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_small_and_full_patterns_preserve_bits_without_assigning_a_key() {
        let light = observe(0xFE0A_B123, 1);
        assert_eq!(light.pattern, RawFormIdPattern::SmallMarkerPattern);
        assert_eq!(light.raw, 0xFE0A_B123);
        assert_eq!(light.small_selector_candidate, Some(0x0AB));
        assert_eq!(light.small_low_12_candidate, Some(0x123));
        assert_eq!(light.full_selector_outside_master_list, None);

        let full = observe(0x0200_0801, 3);
        assert_eq!(full.pattern, RawFormIdPattern::FullWidthPattern);
        assert_eq!(full.raw_high_byte, 2);
        assert_eq!(full.full_low_24_candidate, 0x0000_0801);
        assert_eq!(full.full_selector_outside_master_list, Some(false));

        let source_local = observe(0x0000_0801, 0);
        assert_eq!(source_local.full_selector_outside_master_list, Some(true));
    }

    #[test]
    fn zero_and_medium_markers_are_explicit_without_fo4_resolution() {
        let zero = observe(0, 0);
        assert_eq!(zero.pattern, RawFormIdPattern::ZeroValue);
        assert_eq!(zero.raw, 0);
        let medium = observe(0xFD12_3456, 8);
        assert_eq!(medium.pattern, RawFormIdPattern::MediumMarkerPattern);
        assert_eq!(medium.medium_selector_candidate, Some(0x12));
        assert_eq!(medium.medium_low_16_candidate, Some(0x3456));
        assert_eq!(medium.full_selector_outside_master_list, None);
    }

    #[test]
    fn fallout_four_small_flag_is_observed_without_changing_the_raw_id() {
        assert!(!is_small_master_flag(0x81));
        assert!(is_small_master_flag(0x281));
        assert_eq!(observe(0xFE00_0801, 1).raw, 0xFE00_0801);
    }
}
