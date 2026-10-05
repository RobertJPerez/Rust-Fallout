use fallout4_prep::vmad::{self, Limits, Value};

fn string(b: &mut Vec<u8>, s: &[u8]) {
    b.extend((s.len() as u16).to_le_bytes());
    b.extend(s);
}
fn header(format: u16, scripts: u16) -> Vec<u8> {
    [
        6u16.to_le_bytes(),
        format.to_le_bytes(),
        scripts.to_le_bytes(),
    ]
    .concat()
}
fn script(b: &mut Vec<u8>, count: u16) {
    string(b, b"Probe");
    b.push(1);
    b.extend(count.to_le_bytes());
}
fn prop(b: &mut Vec<u8>, kind: u8) {
    string(b, b"Field");
    b.extend([kind, 3]);
}

#[test]
fn preserves_nested_values_float_bits_raw_names_and_noncanonical_bools() {
    let mut b = header(2, 1);
    script(&mut b, 1);
    prop(&mut b, 17);
    b.extend(1u32.to_le_bytes());
    b.extend(3u32.to_le_bytes());
    prop(&mut b, 4);
    b.extend(0x7FC0_0042u32.to_le_bytes());
    prop(&mut b, 2);
    string(&mut b, b"\xff\0raw");
    prop(&mut b, 5);
    b.push(255);
    let a = vmad::parse(&b, *b"ACTI", "fixture", Limits::default()).unwrap();
    let Value::Structs(v) = &a.scripts[0].properties[0].value else {
        panic!()
    };
    assert!(matches!(v[0][0].value, Value::FloatBits(0x7FC0_0042)));
    assert!(matches!(v[0][1].value, Value::String(b"\xff\0raw")));
    assert!(matches!(v[0][2].value, Value::BoolByte(255)));
    assert_eq!(a.scripts[0].range.end, b.len());
    assert_eq!(a.raw, b);
    // Every proper truncation inside this single-script payload must fail.
    for end in 0..b.len() {
        assert!(
            vmad::parse(&b[..end], *b"ACTI", "cut", Limits::default()).is_err(),
            "{end}"
        );
    }
}
#[test]
fn quest_alias_reads_its_own_format_and_preserves_index_bits() {
    let mut b = header(2, 0);
    b.push(3);
    b.extend(1u16.to_le_bytes());
    string(&mut b, b"");
    b.extend(0xA123_000Au32.to_le_bytes());
    b.extend(0xF012_4567u32.to_le_bytes());
    b.push(0xFE);
    string(&mut b, b"FragmentClass");
    string(&mut b, b"Run");
    b.extend(1u16.to_le_bytes());
    b.extend(0x0100_1234u32.to_le_bytes());
    b.extend((-2i16).to_le_bytes());
    b.extend(0xABCDu16.to_le_bytes());
    b.extend(6u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    script(&mut b, 1);
    prop(&mut b, 1);
    b.extend(0x0200_5678u32.to_le_bytes());
    b.extend((-1i16).to_le_bytes());
    b.extend(0x1234u16.to_le_bytes());
    let a = vmad::parse(&b, *b"QUST", "alias", Limits::default()).unwrap();
    let f = a.fragments.unwrap();
    assert_eq!(f.fragments[0].index_bits, 0xA123_000A);
    assert_eq!(f.fragments[0].stage_index_bits, Some(0xF012_4567));
    let alias = &f.aliases[0];
    assert_eq!(alias.object_format, 1);
    assert_eq!(alias.object.form_id, 0x0100_1234);
    assert_eq!(alias.object.alias, -2);
    assert_eq!(alias.object.unused, 0xABCD);
    let Value::Object(o) = &alias.scripts[0].properties[0].value else {
        panic!()
    };
    assert_eq!(o.form_id, 0x0200_5678);
    assert_eq!(o.unused, 0x1234);
}
#[test]
fn event_bits_determine_order_and_scene_phase_framing() {
    for (kind, flags) in [(*b"INFO", 3), (*b"PACK", 5), (*b"SCEN", 2)] {
        let mut b = header(2, 0);
        b.extend([3, flags]);
        script(&mut b, 0);
        for bit in 0..3 {
            if flags & (1 << bit) != 0 {
                b.push(7);
                string(&mut b, b"Probe");
                string(&mut b, &[b'A' + bit]);
            }
        }
        if kind == *b"SCEN" {
            b.extend(1u16.to_le_bytes());
            b.push(0x80);
            b.extend(0xFF01_0203u32.to_le_bytes());
            b.push(9);
            string(&mut b, b"Phase");
            string(&mut b, b"Go");
        }
        let a = vmad::parse(&b, kind, "events", Limits::default()).unwrap();
        let f = a.fragments.unwrap();
        assert_eq!(f.flags, Some(flags));
        assert_eq!(
            f.fragments.iter().map(|f| f.index_bits).collect::<Vec<_>>(),
            (0..3).filter(|i| flags & (1 << i) != 0).collect::<Vec<_>>()
        );
        if kind == *b"SCEN" {
            assert_eq!(f.phases[0].index_bits, 0xFF01_0203);
            assert_eq!(f.phases[0].flags, 0x80);
        }
        b.push(0);
        assert!(vmad::parse(&b, kind, "tail", Limits::default()).is_err());
    }
}
#[test]
fn bounded_recursion_and_counts_fail_before_allocation() {
    let mut b = header(2, 1);
    script(&mut b, 1);
    prop(&mut b, 11);
    b.extend(u32::MAX.to_le_bytes());
    assert!(vmad::parse(&b, *b"ACTI", "huge", Limits::default()).is_err());
    let mut b = header(2, 1);
    script(&mut b, 1);
    for _ in 0..40 {
        prop(&mut b, 7);
        b.extend(1u32.to_le_bytes());
    }
    prop(&mut b, 0);
    assert!(
        vmad::parse(&b, *b"ACTI", "deep", Limits::default())
            .unwrap_err()
            .to_string()
            .contains("nesting")
    );
    assert!(
        vmad::parse(
            &header(2, 0),
            *b"ACTI",
            "tiny",
            Limits {
                bytes: 5,
                ..Limits::default()
            }
        )
        .is_err()
    );
    assert!(
        vmad::parse(
            &b,
            *b"ACTI",
            "nodes",
            Limits {
                nodes: 1,
                ..Limits::default()
            }
        )
        .is_err()
    );
}
#[test]
fn unresolved_dialects_types_and_presence_bits_are_not_guessed() {
    for code in [6, 16, 255] {
        let mut b = header(2, 1);
        script(&mut b, 1);
        prop(&mut b, code);
        assert!(
            vmad::parse(&b, *b"ACTI", "type", Limits::default())
                .unwrap_err()
                .to_string()
                .contains("unsupported")
        );
    }
    assert!(vmad::parse(&header(3, 0), *b"ACTI", "format", Limits::default()).is_err());
    let mut b = header(2, 0);
    b[0] = 7;
    assert!(vmad::parse(&b, *b"ACTI", "version", Limits::default()).is_err());
    let mut b = header(2, 0);
    b.extend([3, 8]);
    assert!(vmad::parse(&b, *b"PACK", "flags", Limits::default()).is_err());
    let mut b = header(2, 1);
    script(&mut b, 1);
    prop(&mut b, 7);
    b.extend(0u32.to_le_bytes());
    b[0] = 5;
    assert!(vmad::parse(&b, *b"ACTI", "v5", Limits::default()).is_err());
}
