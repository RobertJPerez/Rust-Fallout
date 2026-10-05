use fallout4_prep::pex::{self, Limits, Value};

fn u16b(out: &mut Vec<u8>, n: u16) {
    out.extend(n.to_le_bytes());
}
fn u32b(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_le_bytes());
}
fn word(out: &mut Vec<u8>, s: &[u8]) {
    u16b(out, s.len() as u16);
    out.extend(s);
}
// Original minimal FO4 wire fixture. Expected values are explicit, independent
// of the decoder's opcode table and collection routines.
fn fixture(code: &[u8], instruction_count: u16) -> Vec<u8> {
    let mut b = vec![0xde, 0xc0, 0x57, 0xfa, 3, 9, 2, 0];
    b.extend(123u64.to_le_bytes());
    for s in [b"test.psc".as_slice(), b"author", b"computer"] {
        word(&mut b, s);
    }
    u16b(&mut b, 6);
    for s in [
        b"Probe".as_slice(),
        b"ScriptObject",
        b"",
        b"Run",
        b"Int",
        b"value",
    ] {
        word(&mut b, s);
    }
    b.push(0);
    u16b(&mut b, 0);
    u16b(&mut b, 1);
    u16b(&mut b, 0);
    let size_pos = b.len();
    u32b(&mut b, 0);
    let object_start = b.len();
    u16b(&mut b, 1);
    u16b(&mut b, 2);
    b.push(0);
    u32b(&mut b, 0);
    u16b(&mut b, 2);
    u16b(&mut b, 0);
    u16b(&mut b, 1); // no structs, one variable
    u16b(&mut b, 5);
    u16b(&mut b, 4);
    u32b(&mut b, 0);
    b.push(3);
    u32b(&mut b, 7);
    b.push(0);
    u16b(&mut b, 0);
    u16b(&mut b, 1); // no properties, one state
    u16b(&mut b, 2);
    u16b(&mut b, 1);
    u16b(&mut b, 3);
    u16b(&mut b, 4);
    u16b(&mut b, 2);
    u32b(&mut b, 0);
    b.push(0);
    u16b(&mut b, 0);
    u16b(&mut b, 1);
    u16b(&mut b, 5);
    u16b(&mut b, 4);
    u16b(&mut b, instruction_count);
    b.extend(code);
    let size = (b.len() - object_start) as u32;
    b[size_pos..size_pos + 4].copy_from_slice(&size.to_le_bytes());
    b
}

#[test]
fn decodes_source_less_function_and_exact_values() {
    // assign identifier(value), float NaN bits; return integer(-7).
    let code = [
        13, 1, 5, 0, 4, 0x01, 0, 0xc0, 0x7f, 26, 3, 0xf9, 0xff, 0xff, 0xff,
    ];
    let bytes = fixture(&code, 2);
    let p = pex::parse(&bytes, "authored", Limits::default()).unwrap();
    assert_eq!(p.raw, bytes);
    assert_eq!(p.objects[0].variables, 1);
    let variable = &p.objects[0].variable_definitions[0];
    assert_eq!(variable.name, 5);
    assert_eq!(variable.type_name, 4);
    assert_eq!(variable.initial_value, Value::Integer(7));
    assert_eq!(variable.documentation, None);
    assert_eq!(p.objects[0].state_definitions[0].functions, 0..1);
    let f = &p.objects[0].functions[0];
    assert_eq!(f.locals, [(5, 4)]);
    assert_eq!(
        f.instructions[0].arguments,
        [Value::Identifier(5), Value::FloatBits(0x7fc00001)]
    );
    assert_eq!(f.instructions[1].arguments, [Value::Integer(-7)]);
}

#[test]
fn rejects_every_truncated_prefix_and_trailing_data() {
    let bytes = fixture(&[26, 0], 1);
    for n in 0..bytes.len() {
        assert!(
            pex::parse(&bytes[..n], "truncated", Limits::default()).is_err(),
            "prefix {n}"
        );
    }
    let mut b = bytes;
    b.push(0);
    assert!(pex::parse(&b, "trailing", Limits::default()).is_err());
}

#[test]
fn rejects_other_dialects_and_bad_string_indices() {
    let bytes = fixture(&[26, 1, 0xff, 0xff], 1);
    assert!(pex::parse(&bytes, "index", Limits::default()).is_err());
    for (offset, value) in [(0, 0xfa), (4, 4), (5, 15), (6, 3)] {
        let mut bytes = fixture(&[26, 0], 1);
        bytes[offset] = value;
        assert!(pex::parse(&bytes, "dialect", Limits::default()).is_err());
    }
}

#[test]
fn rejects_vararg_allocation_attacks_and_future_opcodes() {
    for code in [
        vec![47],
        vec![255],
        vec![23, 1, 3, 0, 1, 0, 0, 0, 3, 0xff, 0xff, 0xff, 0xff],
        vec![23, 1, 3, 0, 1, 0, 0, 0, 0],
    ] {
        assert!(pex::parse(&fixture(&code, 1), "attack", Limits::default()).is_err());
    }
    let limits = Limits {
        nodes: 2,
        ..Limits::default()
    };
    assert!(pex::parse(&fixture(&[26, 0], 1), "budget", limits).is_err());
}

#[test]
fn branches_use_instruction_offsets_with_bounds() {
    let good = fixture(&[20, 3, 1, 0, 0, 0, 26, 0], 2);
    assert!(pex::parse(&good, "jump", Limits::default()).is_ok());
    for delta in [3i32, -1] {
        let mut code = vec![20, 3];
        code.extend(delta.to_le_bytes());
        code.extend([26, 0]);
        assert!(pex::parse(&fixture(&code, 2), "jump", Limits::default()).is_err());
    }
}

#[test]
fn validates_retail_and_caprica_object_size_conventions() {
    let original = fixture(&[26, 0], 1);
    let parsed = pex::parse(&original, "caprica-layout", Limits::default()).unwrap();
    let offset = parsed.objects[0].range.start + 2;
    let body_size = parsed.objects[0].declared_size;
    assert_eq!(parsed.objects[0].size_convention, "body-only");
    let mut retail = original.clone();
    retail[offset..offset + 4].copy_from_slice(&(body_size + 4).to_le_bytes());
    assert_eq!(
        pex::parse(&retail, "retail-layout", Limits::default())
            .unwrap()
            .objects[0]
            .size_convention,
        "includes-size-field"
    );
    for size in [0, body_size - 1, body_size + 1, u32::MAX] {
        let mut bad = original.clone();
        bad[offset..offset + 4].copy_from_slice(&size.to_le_bytes());
        assert!(pex::parse(&bad, "bad-size", Limits::default()).is_err());
    }
}

#[test]
fn retains_struct_members_auto_backing_and_accessor_ownership() {
    let original = fixture(&[26, 0], 1);
    let parsed = pex::parse(&original, "base", Limits::default()).unwrap();
    let start = parsed.objects[0].range.start;
    let mut b = original[..start + 17].to_vec(); // through auto-state field
    u16b(&mut b, 1);
    u16b(&mut b, 0);
    u16b(&mut b, 1); // struct, name, members
    u16b(&mut b, 5);
    u16b(&mut b, 4);
    u32b(&mut b, 0x1234);
    b.push(3);
    u32b(&mut b, (-11i32) as u32);
    b.push(1);
    u16b(&mut b, 2);
    u16b(&mut b, 1);
    u16b(&mut b, 5);
    u16b(&mut b, 4);
    u32b(&mut b, 0);
    b.push(3);
    u32b(&mut b, 7);
    b.push(0);
    u16b(&mut b, 2); // two properties
    u16b(&mut b, 3);
    u16b(&mut b, 4);
    u16b(&mut b, 2);
    u32b(&mut b, 32);
    b.push(7);
    u16b(&mut b, 5);
    u16b(&mut b, 0);
    u16b(&mut b, 4);
    u16b(&mut b, 2);
    u32b(&mut b, 64);
    b.push(3);
    for setter in [false, true] {
        u16b(&mut b, if setter { 2 } else { 4 });
        u16b(&mut b, 2);
        u32b(&mut b, 0);
        b.push(0);
        u16b(&mut b, u16::from(setter));
        if setter {
            u16b(&mut b, 5);
            u16b(&mut b, 4);
        }
        u16b(&mut b, 0);
        u16b(&mut b, 1);
        b.push(26);
        if setter {
            b.push(0);
        } else {
            b.push(3);
            u32b(&mut b, 11);
        }
    }
    u16b(&mut b, 1);
    u16b(&mut b, 2);
    u16b(&mut b, 0); // empty default state
    let size = (b.len() - start - 6) as u32;
    b[start + 2..start + 6].copy_from_slice(&size.to_le_bytes());
    let parsed = pex::parse(&b, "definitions", Limits::default()).unwrap();
    let o = &parsed.objects[0];
    let m = &o.struct_definitions[0].members[0];
    assert_eq!(m.initial_value, Value::Integer(-11));
    assert_eq!(m.constant, 1);
    assert_eq!(m.documentation, Some(2));
    assert_eq!(m.user_flags, 0x1234);
    assert_eq!(o.property_definitions[0].auto_variable, Some(5));
    assert_eq!(o.property_definitions[0].getter, None);
    assert_eq!(o.property_definitions[1].getter, Some(0));
    assert_eq!(o.property_definitions[1].setter, Some(1));
    assert_eq!(o.functions[1].parameters, [(5, 4)]);
    assert_eq!(o.state_definitions[0].functions, 2..2);
    assert_eq!(
        o.property_definitions[1].range.end,
        o.state_definitions[0].range.start - 2
    );
    for end in 0..b.len() {
        assert!(
            pex::parse(&b[..end], "cut", Limits::default()).is_err(),
            "{end}"
        );
    }
}
