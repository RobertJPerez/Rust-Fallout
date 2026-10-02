//! Original synthetic container bytes, independent of any retail model.
use fallout_data::nif;

fn fixture() -> Vec<u8> {
    let mut b = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    b.extend(0x1402_0007u32.to_le_bytes());
    b.push(1);
    for n in [11u32, 2, 34] {
        b.extend(n.to_le_bytes());
    }
    b.extend([0; 3]); // Empty export strings.
    b.extend(2u16.to_le_bytes());
    for name in [b"NiNode".as_slice(), b"UnimplementedBlock"] {
        b.extend((name.len() as u32).to_le_bytes());
        b.extend(name);
    }
    b.extend(0u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(3u32.to_le_bytes());
    b.extend(2u32.to_le_bytes());
    b.extend(1u32.to_le_bytes());
    b.extend(4u32.to_le_bytes());
    b.extend(4u32.to_le_bytes());
    b.extend(b"name");
    b.extend(0u32.to_le_bytes()); // No groups.
    b.extend([9, 8, 7, 6, 5]);
    b.extend(2u32.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b.extend(u32::MAX.to_le_bytes());
    b
}

#[test]
fn inventories_unknown_blocks_without_discarding_their_bytes() {
    let bytes = fixture();
    let index = nif::inspect(&bytes, "original fixture").unwrap();
    assert_eq!(index.blocks.len(), 2);
    assert_eq!(index.block_counts["UnimplementedBlock"], 1);
    let block = &index.blocks[1];
    assert_eq!(&bytes[block.offset..block.offset + block.bytes], &[6, 5]);
    assert_eq!(index.roots, [Some(0), None]);
    assert_eq!(index.strings, [b"name".to_vec()]);
}

#[test]
fn all_truncated_prefixes_and_bad_roots_fail() {
    let mut bytes = fixture();
    for length in 0..bytes.len() {
        assert!(
            nif::inspect(&bytes[..length], "truncated").is_err(),
            "{length}"
        );
    }
    let root = bytes.len() - 8;
    bytes[root..root + 4].copy_from_slice(&2u32.to_le_bytes());
    assert!(nif::inspect(&bytes, "bad root").is_err());
}

#[test]
fn version_dispatch_and_count_budgets_are_enforced() {
    let original = fixture();
    let line = original.iter().position(|b| *b == b'\n').unwrap() + 1;
    for (offset, value) in [(line + 5, 12u32), (line + 9, u32::MAX), (line + 13, 83)] {
        let mut bytes = original.clone();
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(nif::inspect(&bytes, "unsupported/bomb").is_err());
    }
    for offset in 0..original.len() {
        let mut bytes = original.clone();
        bytes[offset] = 255;
        let _ = nif::inspect(&bytes, "mutation sweep");
    }
}

#[test]
fn observed_nv_stream_revisions_keep_their_identity() {
    let original = fixture();
    let line = original.iter().position(|b| *b == b'\n').unwrap() + 1;
    for version in [14u32, 21, 24, 25, 26, 27, 28, 30, 31, 32, 33, 34] {
        let mut bytes = original.clone();
        bytes[line + 13..line + 17].copy_from_slice(&version.to_le_bytes());
        let index = nif::inspect(&bytes, "stream fixture").unwrap();
        assert_eq!(index.bethesda_version, version);
    }
}
