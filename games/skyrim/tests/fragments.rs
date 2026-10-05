use skyrim_prep::vmad::{self, Limits, Selector, Tail, Value};

fn text(out: &mut Vec<u8>, s: &[u8]) {
    out.extend((s.len() as u16).to_le_bytes());
    out.extend(s);
}
fn root() -> Vec<u8> {
    vec![5, 0, 2, 0, 0, 0]
}
fn binding(out: &mut Vec<u8>, script: &[u8], function: &[u8]) {
    out.push(0xA5);
    text(out, script);
    text(out, function);
}
fn quest() -> Vec<u8> {
    let mut b = root();
    b.extend([2, 1, 0]);
    text(&mut b, b"Q");
    b.extend([0x78, 0x56, 0x34, 0x12, 0xCD, 0xAB, 0, 0]);
    binding(&mut b, b"Q", b"Run");
    b.extend([1, 0]); // One alias; outer object format 2.
    b.extend([
        0x11, 0x22, 0xFE, 0xFF, 0x44, 0x33, 0x22, 0x11, 5, 0, 2, 0, 1, 0,
    ]);
    text(&mut b, b"AliasScript");
    b.extend([0, 1, 0]);
    text(&mut b, b"P");
    b.extend([4, 1, 0x34, 0x12, 0xC0, 0x7F]);
    b
}
#[test]
fn quest_stage_log_alias_and_exact_property_bits_survive() {
    let bytes = quest();
    let a = vmad::decode_record(&bytes, *b"QUST", "quest", Limits::default()).unwrap();
    let Tail::Decoded(t) = a.tail else {
        panic!("{:?}", a.tail);
    };
    assert_eq!(
        t.fragments[0].selector,
        Selector::Quest {
            stage: 0x12345678,
            log_entry: 0xABCD
        }
    );
    assert_eq!(t.fragments[0].function_name, b"Run");
    assert_eq!(t.fragments[0].unknown, 0xA5);
    assert_eq!(t.aliases[0].object.form_id, 0x11223344);
    assert_eq!(t.aliases[0].object.alias, -2);
    assert_eq!(t.aliases[0].object.unused, [0x11, 0x22]);
    assert_eq!(t.aliases[0].scripts[0].name, b"AliasScript");
    assert_eq!(
        t.aliases[0].scripts[0].properties[0].value,
        Value::FloatBits(0x7FC01234)
    );
    assert_eq!(t.offset, 6);
    assert_eq!(t.end, bytes.len());
}
#[test]
fn info_and_package_flags_preserve_event_order() {
    for (kind, flags, bits) in [(*b"INFO", 3, vec![0, 1]), (*b"PACK", 5, vec![0, 2])] {
        let mut b = root();
        b.extend([2, flags]);
        text(&mut b, b"F");
        binding(&mut b, b"F", b"SecondByName");
        binding(&mut b, b"F", b"FirstByName");
        let a = vmad::decode_record(&b, kind, "flags", Limits::default()).unwrap();
        let Tail::Decoded(t) = a.tail else {
            panic!("{:?}", a.tail);
        };
        assert_eq!(t.fragments[0].selector, Selector::FlagBit(bits[0]));
        assert_eq!(t.fragments[1].selector, Selector::FlagBit(bits[1]));
        assert_eq!(t.fragments[0].function_name, b"SecondByName");
        assert_eq!(t.flags, Some(flags));
    }
}
#[test]
fn perk_indices_and_scene_phases_are_distinct() {
    let mut b = root();
    b.push(2);
    text(&mut b, b"P");
    b.extend([1, 0, 0x78, 0x56, 0x34, 0x12]);
    binding(&mut b, b"P", b"F");
    let a = vmad::decode_record(&b, *b"PERK", "perk", Limits::default()).unwrap();
    let Tail::Decoded(t) = a.tail else {
        panic!("{:?}", a.tail);
    };
    assert_eq!(t.fragments[0].selector, Selector::PerkIndex(0x12345678));
    let mut b = root();
    b.extend([2, 2]);
    text(&mut b, b"S");
    binding(&mut b, b"S", b"End");
    b.extend([1, 0, 3, 0xEF, 0xCD, 0xAB, 0x89]);
    binding(&mut b, b"S", b"Phase");
    let a = vmad::decode_record(&b, *b"SCEN", "scene", Limits::default()).unwrap();
    let Tail::Decoded(t) = a.tail else {
        panic!("{:?}", a.tail);
    };
    assert_eq!(t.fragments[0].selector, Selector::FlagBit(1));
    assert_eq!(
        t.fragments[1].selector,
        Selector::ScenePhase {
            flags: 3,
            index: 0x89ABCDEF
        }
    );
}
#[test]
fn tail_truncations_and_leftovers_preserve_raw_failure() {
    let b = quest();
    for end in 7..b.len() {
        let a = vmad::decode_record(&b[..end], *b"QUST", "cut", Limits::default()).unwrap();
        assert!(matches!(a.tail, Tail::Unsupported { .. }), "cut={end}");
        if let Tail::Unsupported { offset, bytes, .. } = a.tail {
            assert_eq!(offset, 6);
            assert_eq!(bytes, &b[6..end]);
        }
    }
    let mut b = b;
    b.push(0xDD);
    assert!(matches!(
        vmad::decode_record(&b, *b"QUST", "extra", Limits::default())
            .unwrap()
            .tail,
        Tail::Unsupported { .. }
    ));
}
#[test]
fn aggregate_budget_spans_primary_fragment_and_alias_sections() {
    let b = quest();
    let limits = Limits {
        max_items: 3,
        ..Limits::default()
    }; // fragment+alias+script exhaust before property
    assert!(matches!(
        vmad::decode_record(&b, *b"QUST", "budget", limits)
            .unwrap()
            .tail,
        Tail::Unsupported { .. }
    ));
    let limits = Limits {
        max_items: 4,
        ..Limits::default()
    };
    assert!(matches!(
        vmad::decode_record(&b, *b"QUST", "budget", limits)
            .unwrap()
            .tail,
        Tail::Decoded(_)
    ));
}
#[test]
fn unsupported_version_flags_kind_and_alias_layout_do_not_guess() {
    let b = quest();
    assert!(matches!(
        vmad::decode_record(&b, *b"ACTI", "kind", Limits::default())
            .unwrap()
            .tail,
        Tail::Unsupported { .. }
    ));
    let mut future = b.clone();
    future[6] = 3;
    assert!(matches!(
        vmad::decode_record(&future, *b"QUST", "version", Limits::default())
            .unwrap()
            .tail,
        Tail::Unsupported { .. }
    ));
    let mut flags = root();
    flags.extend([2, 0x80, 0, 0]);
    assert!(matches!(
        vmad::decode_record(&flags, *b"INFO", "flags", Limits::default())
            .unwrap()
            .tail,
        Tail::Unsupported { .. }
    ));
    let decoded = vmad::decode_record(&b, *b"QUST", "alias", Limits::default()).unwrap();
    let Tail::Decoded(t) = decoded.tail else {
        panic!()
    };
    let alias_at = t.aliases[0].offset;
    let mut mixed = b;
    mixed[alias_at + 10] = 1;
    assert!(matches!(
        vmad::decode_record(&mixed, *b"QUST", "mixed", Limits::default())
            .unwrap()
            .tail,
        Tail::Unsupported { .. }
    ));
}
