use fallout4_prep::archive::{Ba2, Limits};
// Literal BA2 GNRL index with one uncompressed file. No library writer is used.
fn general() -> Vec<u8> {
    let mut b = Vec::from(*b"BTDX");
    b.extend(1u32.to_le_bytes());
    b.extend(b"GNRL");
    b.extend(1u32.to_le_bytes());
    b.extend(65u64.to_le_bytes());
    b.extend([0u8; 12]);
    b.extend([0, 1, 16, 0]);
    b.extend(60u64.to_le_bytes());
    b.extend(0u32.to_le_bytes());
    b.extend(5u32.to_le_bytes());
    b.extend(0xbaadf00du32.to_le_bytes());
    b.extend(b"hello");
    let path = b"Scripts/Test.pex";
    b.extend((path.len() as u16).to_le_bytes());
    b.extend(path);
    b
}
#[test]
fn reuses_shared_paths_and_reads_by_index() {
    let archive = Ba2::from_bytes(general(), Limits::default()).unwrap();
    assert_eq!(archive.find(b"SCRIPTS\\TEST.PEX").unwrap(), Some(0));
    assert_eq!(archive.read(0).unwrap(), b"hello");
    assert!(archive.find(b"../bad").is_err());
    assert!(archive.read(1).is_err());
}
#[test]
fn rejects_metadata_and_decompression_budgets() {
    for limits in [
        Limits {
            entries: 0,
            ..Default::default()
        },
        Limits {
            chunks: 0,
            ..Default::default()
        },
        Limits {
            name_bytes: 1,
            ..Default::default()
        },
    ] {
        assert!(Ba2::from_bytes(general(), limits).is_err());
    }
    let archive = Ba2::from_bytes(
        general(),
        Limits {
            member_bytes: 4,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(archive.read(0).is_err());
    let mut bytes = general();
    bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(Ba2::from_bytes(bytes, Limits::default()).is_err());
}
#[test]
fn rejects_truncation_bad_offsets_versions_and_paths() {
    let bytes = general();
    for n in 0..bytes.len() {
        assert!(
            Ba2::from_bytes(bytes[..n].to_vec(), Limits::default()).is_err(),
            "{n}"
        );
    }
    let mut bytes = general();
    bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
    assert!(Ba2::from_bytes(bytes, Limits::default()).is_err());
    let mut bytes = general();
    bytes[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(Ba2::from_bytes(bytes, Limits::default()).is_err());
    let mut bytes = general();
    bytes[67..70].copy_from_slice(b"../");
    assert!(Ba2::from_bytes(bytes, Limits::default()).is_err());
}

#[test]
fn rejects_payloads_overlapping_metadata() {
    for offset in [0u64, 24, 65] {
        let mut bytes = general();
        bytes[40..48].copy_from_slice(&offset.to_le_bytes());
        assert!(Ba2::from_bytes(bytes, Limits::default()).is_err());
    }
}

#[test]
fn validates_zlib_integrity_and_exact_decoded_size() {
    // Independently specified zlib stream for the five bytes "hello".
    let zlib = [
        0x78, 0x9c, 0xcb, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00, 0x06, 0x2c, 0x02, 0x15,
    ];
    let mut bytes = general();
    bytes.splice(60..65, zlib);
    bytes[16..24].copy_from_slice(&73u64.to_le_bytes());
    bytes[48..52].copy_from_slice(&13u32.to_le_bytes());
    assert_eq!(
        Ba2::from_bytes(bytes.clone(), Limits::default())
            .unwrap()
            .read(0)
            .unwrap(),
        b"hello"
    );
    let mut corrupt = bytes.clone();
    corrupt[72] ^= 1;
    assert!(
        Ba2::from_bytes(corrupt, Limits::default())
            .unwrap()
            .read(0)
            .is_err()
    );
    bytes[52..56].copy_from_slice(&4u32.to_le_bytes());
    assert!(
        Ba2::from_bytes(bytes, Limits::default())
            .unwrap()
            .read(0)
            .is_err()
    );
}
