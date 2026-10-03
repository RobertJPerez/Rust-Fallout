use fallout_data::{
    condition::{self, ComparisonOperator, ComparisonValue, RunOnDomain},
    condition_census,
};
use std::fs;

#[test]
fn short_layouts_keep_absent_words_padding_and_exact_union_bits() {
    let mut bytes = [0; 28];
    bytes[0] = 0xa1;
    bytes[1..4].copy_from_slice(&[1, 2, 3]);
    bytes[4..8].copy_from_slice(&0x8000_0000_u32.to_le_bytes());
    bytes[8..10].copy_from_slice(&106_u16.to_le_bytes());
    bytes[10..12].copy_from_slice(&[4, 5]);
    bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[16..20].copy_from_slice(&0x7fc0_1234_u32.to_le_bytes());
    bytes[20..24].copy_from_slice(&2_u32.to_le_bytes());
    bytes[24..28].copy_from_slice(&0x123_u32.to_le_bytes());
    for size in [20, 24, 28] {
        let decoded = condition::decode(&bytes[..size]).unwrap();
        assert!(std::ptr::eq(decoded.bytes.as_ptr(), bytes.as_ptr()));
        assert_eq!(
            decoded.comparison_operator(),
            ComparisonOperator::LessOrEqual
        );
        assert_eq!(
            decoded.comparison_value(),
            ComparisonValue::FloatBits(0x8000_0000)
        );
        assert_eq!(decoded.parameter_words, [u32::MAX, 0x7fc0_1234]);
        assert_eq!(decoded.flag_padding, [1, 2, 3]);
        assert_eq!(decoded.function_padding, [4, 5]);
        assert_eq!(decoded.run_on_word, (size >= 24).then_some(2));
        assert_eq!(decoded.reference_word, (size >= 28).then_some(0x123));
        assert_eq!(decoded.run_on_domain(), RunOnDomain::AnimationGroup);
        assert!(!decoded.reference_is_subject_selector());
        assert!(decoded.or_flag());
    }
    bytes[0] = 4;
    bytes[8..10].copy_from_slice(&1_u16.to_le_bytes());
    let decoded = condition::decode(&bytes).unwrap();
    assert_eq!(
        decoded.comparison_value(),
        ComparisonValue::GlobalRawForm(0x8000_0000)
    );
    assert!(decoded.reference_is_subject_selector());
    assert_eq!(decoded.finite_float_comparison_word(), None);
}

#[test]
fn all_flag_words_keep_unknown_comparisons_and_nonfinite_patterns_visible() {
    let mut bytes = [0; 20];
    bytes[4..8].copy_from_slice(&0x7fc0_1234_u32.to_le_bytes());
    for flags in 0..=u8::MAX {
        bytes[0] = flags;
        let decoded = condition::decode(&bytes).unwrap();
        assert_eq!(decoded.flags, flags);
        assert_eq!(decoded.or_flag(), flags & 1 != 0);
        assert_eq!(
            decoded.finite_float_comparison_word(),
            if flags & 4 != 0 { None } else { Some(false) }
        );
        if flags >> 5 > 5 {
            assert_eq!(
                decoded.comparison_operator(),
                ComparisonOperator::Unknown(flags >> 5)
            );
        }
    }
    for size in 0..=40 {
        let buffer = vec![0; size];
        assert_eq!(
            condition::decode(&buffer).is_ok(),
            [20, 24, 28].contains(&size)
        );
    }
}

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(bytes.len() as u32).to_le_bytes(),
        &[0; 4],
        &1_u32.to_le_bytes(),
        &[0; 8],
        bytes,
    ]
    .concat()
}

#[test]
fn authored_condition_order_and_preceding_fields_do_not_become_guessed_groups() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("Fixture.esm");
    let mut first = [0; 20];
    first[0] = 1;
    first[8] = 1;
    let mut second = [0; 24];
    second[0] = 0xe4;
    second[8] = 2;
    second[20] = 2;
    fs::write(
        &path,
        [
            record(b"TES4", &[]),
            record(
                b"INFO",
                &[
                    field(b"EDID", b"p\0"),
                    field(b"CTDA", &first),
                    field(b"CTDA", &second),
                ]
                .concat(),
            ),
            record(b"LAND", &[0xff]),
        ]
        .concat(),
    )
    .unwrap();
    let mut observed = 0;
    let report = condition_census::inspect(&path, true, |_| {
        observed += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(observed, 1);
    assert_eq!(report.rows.len(), 2);
    assert_eq!(report.rows[0].preceding_field_kind.as_deref(), Some("EDID"));
    assert_eq!(report.rows[0].field_decoded_offset, 8);
    assert_eq!(report.rows[1].preceding_field_kind.as_deref(), Some("CTDA"));
    assert_eq!(report.rows[1].preceding_field_decoded_offset, Some(8));
    assert_eq!(report.rows[1].field_decoded_offset, 34);
    assert_eq!(report.counts.conditions, 2);
    assert_eq!(report.counts.absent_run_on_words, 1);
    assert_eq!(report.counts.absent_reference_words, 2);
    assert_eq!(report.counts.active_reference_words, 0);
    assert_eq!(report.counts.unknown_comparison_operators, 1);
    assert_eq!(report.record_payloads_deferred, 1);
    assert!(!report.evaluation_ready);
    assert!(!report.retail_parity_accepted);
    assert!(condition_census::inspect(&path, false, |_| Ok(())).is_err());
}
