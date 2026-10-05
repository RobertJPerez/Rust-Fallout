//! Minimal Papyrus PEX dialect-prefix observation; deliberately not a decoder.
use serde::Serialize;

const SKYRIM_PEX_MAGIC: [u8; 4] = [0xFA, 0x57, 0xC0, 0xDE];
const LITTLE_ENDIAN_PEX_MAGIC: [u8; 4] = [0xDE, 0xC0, 0x57, 0xFA];
const SKYRIM_PEX_VERSION: (u8, u8, u16) = (3, 2, 1);
const DIALECT_PREFIX_BYTES: usize = 8;

#[derive(Debug, Clone, Serialize)]
pub struct Observation {
    pub status: &'static str,
    pub dialect_prefix_bytes: Vec<u8>,
    pub endianness: Option<&'static str>,
    pub major: Option<u8>,
    pub minor: Option<u8>,
    pub game: Option<u16>,
}

impl Observation {
    pub fn census_key(&self) -> String {
        match (self.major, self.minor, self.game) {
            (Some(major), Some(minor), Some(game)) => format!(
                "{}-endian/{major}.{minor}/game-{game}",
                self.endianness.unwrap_or("unknown")
            ),
            _ => self.status.to_owned(),
        }
    }

    pub fn is_skyrim(&self) -> bool {
        self.status == "skyrim-big-endian-prefix-observed"
    }
}

/// Keeps only the PEX magic and version tuple prefix. The remaining header and body are not parsed.
pub fn observe(bytes: &[u8]) -> Observation {
    let dialect_prefix_bytes = bytes[..bytes.len().min(DIALECT_PREFIX_BYTES)].to_vec();
    if bytes.len() < DIALECT_PREFIX_BYTES {
        return Observation {
            status: "truncated-dialect-prefix",
            dialect_prefix_bytes,
            endianness: None,
            major: None,
            minor: None,
            game: None,
        };
    }
    let (magic_status, endianness) = if bytes[..4] == SKYRIM_PEX_MAGIC {
        ("skyrim-big-endian-prefix-observed", "big")
    } else if bytes[..4] == LITTLE_ENDIAN_PEX_MAGIC {
        ("other-little-endian-prefix-observed", "little")
    } else {
        return Observation {
            status: "unexpected-magic",
            dialect_prefix_bytes,
            endianness: None,
            major: None,
            minor: None,
            game: None,
        };
    };
    let game_bytes = [bytes[6], bytes[7]];
    let game = if endianness == "big" {
        u16::from_be_bytes(game_bytes)
    } else {
        u16::from_le_bytes(game_bytes)
    };
    let tuple = (bytes[4], bytes[5], game);
    let status =
        if magic_status == "skyrim-big-endian-prefix-observed" && tuple != SKYRIM_PEX_VERSION {
            "unsupported-skyrim-tuple"
        } else {
            magic_status
        };
    Observation {
        status,
        dialect_prefix_bytes,
        endianness: Some(endianness),
        major: Some(bytes[4]),
        minor: Some(bytes[5]),
        game: Some(game),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_only_the_exact_dialect_prefix_and_tuple() {
        let mut bytes = SKYRIM_PEX_MAGIC.to_vec();
        bytes.extend([3, 2, 0, 1]);
        bytes.extend([0xAA, 0xBB]);
        let observed = observe(&bytes);
        assert_eq!(observed.status, "skyrim-big-endian-prefix-observed");
        assert!(observed.is_skyrim());
        assert_eq!(observed.dialect_prefix_bytes, bytes[..DIALECT_PREFIX_BYTES]);
        assert_eq!(
            (observed.major, observed.minor, observed.game),
            (Some(3), Some(2), Some(1))
        );
        assert_eq!(observed.census_key(), "big-endian/3.2/game-1");
    }

    #[test]
    fn classifies_other_endian_and_preserves_invalid_prefixes() {
        let other = observe(&[0xDE, 0xC0, 0x57, 0xFA, 3, 9, 2, 0]);
        assert_eq!(other.status, "other-little-endian-prefix-observed");
        assert!(!other.is_skyrim());
        assert_eq!(other.census_key(), "little-endian/3.9/game-2");

        let unsupported = observe(&[0xFA, 0x57, 0xC0, 0xDE, 3, 3, 0, 1]);
        assert_eq!(unsupported.status, "unsupported-skyrim-tuple");
        assert!(!unsupported.is_skyrim());
        assert_eq!(unsupported.census_key(), "big-endian/3.3/game-1");

        let short = observe(&SKYRIM_PEX_MAGIC[..3]);
        assert_eq!(short.status, "truncated-dialect-prefix");
        assert_eq!(short.dialect_prefix_bytes, SKYRIM_PEX_MAGIC[..3]);
        assert_eq!(short.census_key(), "truncated-dialect-prefix");

        let unknown = observe(b"not a PEX");
        assert_eq!(unknown.status, "unexpected-magic");
        assert_eq!(unknown.dialect_prefix_bytes, b"not a PE");
        assert_eq!(unknown.census_key(), "unexpected-magic");
        assert_eq!(
            (unknown.major, unknown.minor, unknown.game),
            (None, None, None)
        );
        assert!(!unknown.is_skyrim());
    }
}
