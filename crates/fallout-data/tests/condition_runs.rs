use fallout_data::{
    condition_operands::{
        self, OwnerLimits, PreparedOwnerRecord, RecordLimits, RunLimits, Signatures,
    },
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
};
use serde_json::{Value, json};
use std::fs;

fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
fn record(kind: &[u8; 4], id: u32, body: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(body.len() as u32).to_le_bytes(),
        &[0; 4],
        &id.to_le_bytes(),
        &[0; 8],
        body,
    ]
    .concat()
}
fn header() -> Vec<u8> {
    record(
        b"TES4",
        0,
        &field(
            b"HEDR",
            &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
        ),
    )
}
fn condition(size: usize, flags: u8) -> Vec<u8> {
    let mut bytes = vec![0; size];
    bytes[0] = flags;
    bytes[8..10].copy_from_slice(&65535_u16.to_le_bytes());
    field(b"CTDA", &bytes)
}
fn prepare(kind: &[u8; 4], body: &[u8], other: bool) -> PreparedOwnerRecord {
    let directory = tempfile::tempdir().unwrap();
    fs::write(
        directory.path().join("FalloutNV.esm"),
        [header(), record(kind, 0x100, body)].concat(),
    )
    .unwrap();
    let mut names = vec!["FalloutNV.esm".into()];
    if other {
        fs::write(directory.path().join("Other.esm"), header()).unwrap();
        names.push("Other.esm".into());
    }
    let mut store =
        RecordStore::open_nv_headers(directory.path(), &names, plugin::Limits::default()).unwrap();
    let location = store
        .winner(&FormKey {
            profile: ProfileId::NvOriginal,
            origin_plugin: "falloutnv.esm".into(),
            local_id: 0x100,
        })
        .unwrap();
    condition_operands::prepare_record_with_owners(
        &mut store,
        location,
        &Signatures::new(),
        RecordLimits::default(),
        OwnerLimits::default(),
    )
    .unwrap()
}
fn projection(prepared: &PreparedOwnerRecord) -> Value {
    serde_json::to_value(
        condition_operands::prepare_source_runs(prepared, RunLimits::default()).unwrap(),
    )
    .unwrap()
}
fn row(owner: usize, first: usize, end: usize, flag: bool, reason: &str) -> Value {
    json!({"owner_section":owner,"first_site":first,"end_site_exclusive":end,"tail_or_flag":flag,"end_reason":reason})
}

#[test]
fn exact_raw_or_bits_describe_adjacent_physical_spans_without_truth() {
    let prepared = prepare(
        b"QUST",
        &[
            condition(20, 1),
            condition(24, 1),
            condition(28, 0),
            condition(20, 0),
            condition(28, 1),
        ]
        .concat(),
        false,
    );
    let before = serde_json::to_value(&prepared).unwrap();
    let report = projection(&prepared);
    assert_eq!(
        report["runs"],
        json!([
            row(0, 0, 3, false, "raw_or_clear"),
            row(0, 3, 4, false, "raw_or_clear"),
            row(0, 4, 5, true, "record_end")
        ])
    );
    for flag in [
        "evaluation_ready",
        "group_evaluation_verified",
        "default_subjects_applied",
    ] {
        assert_eq!(report[flag], false);
    }
    assert_eq!(serde_json::to_value(&prepared).unwrap(), before);
}

#[test]
fn intervening_fields_and_extended_framing_break_even_same_owner_or_tails() {
    let extended = [
        field(b"XXXX", &28_u32.to_le_bytes()),
        b"CTDA\0\0".to_vec(),
        condition(28, 1)[6..].to_vec(),
    ]
    .concat();
    let prepared = prepare(
        b"INFO",
        &[
            condition(20, 255),
            field(b"EDID", b"Gap\0"),
            condition(24, 1),
            extended,
            condition(28, 0),
        ]
        .concat(),
        false,
    );
    let report = projection(&prepared);
    assert_eq!(
        report["runs"],
        json!([
            row(0, 0, 1, true, "physical_field_gap"),
            row(0, 1, 2, true, "physical_field_gap"),
            row(0, 2, 4, false, "raw_or_clear")
        ])
    );
    let sites = prepared.conditions().sites();
    assert_eq!(sites[0].condition().flags, 255);
    assert_eq!(sites[2].preceding_field_kind(), Some("CTDA"));
    assert_ne!(
        sites[2].field_decoded_offset(),
        sites[1].field_decoded_offset() + 6 + sites[1].raw_bytes().len()
    );
}

#[test]
fn owner_changes_orphan_sites_and_record_end_keep_true_tails_explicit() {
    let prepared = prepare(
        b"QUST",
        &[
            condition(20, 1),
            field(b"INDX", &(-7_i16).to_le_bytes()),
            condition(20, 1),
            field(b"QSDT", &[0]),
            condition(28, 1),
            field(b"QSDT", &[0]),
            condition(20, 0),
            field(b"QOBJ", &7_i32.to_le_bytes()),
            condition(20, 1),
            field(b"QSTA", &[0; 8]),
            condition(24, 1),
        ]
        .concat(),
        false,
    );
    let report = projection(&prepared);
    assert_eq!(
        report["runs"],
        json!([
            row(0, 0, 1, true, "unowned_next_site"),
            row(2, 2, 3, true, "owner_change"),
            row(3, 3, 4, false, "raw_or_clear"),
            row(5, 5, 6, true, "record_end")
        ])
    );
    assert_eq!(report["orphan_sites"], 2);
    assert_eq!(report["unmapped_sites"], 0);
}

#[test]
fn unmapped_and_empty_records_acquire_no_runs() {
    let unknown = prepare(
        b"PACK",
        &[condition(20, 1), condition(28, 0)].concat(),
        false,
    );
    let report = projection(&unknown);
    assert_eq!(report["runs"], json!([]));
    assert_eq!(report["unmapped_sites"], 2);
    assert_eq!(report["orphan_sites"], 0);
    let empty = prepare(b"QUST", &[], false);
    assert_eq!(projection(&empty)["runs"], json!([]));
}

#[test]
fn exact_run_count_and_serialized_bytes_are_admitted_and_one_less_rejects() {
    let prepared = prepare(
        b"QUST",
        &[condition(20, 0), condition(28, 0)].concat(),
        false,
    );
    let runs = condition_operands::prepare_source_runs(&prepared, RunLimits::default()).unwrap();
    let bytes = serde_json::to_vec(&runs).unwrap().len();
    assert_eq!(runs.retained_bytes(), bytes);
    assert!(
        condition_operands::prepare_source_runs(
            &prepared,
            RunLimits {
                maximum_runs: 2,
                maximum_retained_bytes: bytes
            }
        )
        .is_ok()
    );
    assert!(
        condition_operands::prepare_source_runs(
            &prepared,
            RunLimits {
                maximum_runs: 1,
                maximum_retained_bytes: bytes
            }
        )
        .unwrap_err()
        .to_string()
        .contains("run count budget")
    );
    assert!(
        condition_operands::prepare_source_runs(
            &prepared,
            RunLimits {
                maximum_runs: 2,
                maximum_retained_bytes: bytes - 1
            }
        )
        .unwrap_err()
        .to_string()
        .contains("run metadata byte budget")
    );
}

#[test]
fn complete_source_cohort_remains_borrowed_even_for_identical_physical_runs() {
    let body = [condition(20, 1), condition(28, 0)].concat();
    let first = prepare(b"INFO", &body, false);
    let changed = prepare(b"INFO", &body, true);
    let a = condition_operands::prepare_source_runs(&first, RunLimits::default()).unwrap();
    let b = condition_operands::prepare_source_runs(&changed, RunLimits::default()).unwrap();
    assert!(std::ptr::eq(a.identity(), first.conditions().identity()));
    assert_eq!(a.identity().decoded_sha256, b.identity().decoded_sha256);
    assert_ne!(
        a.identity().source_cohort_sha256,
        b.identity().source_cohort_sha256
    );
    assert_eq!(
        serde_json::to_value(a.runs()).unwrap(),
        serde_json::to_value(b.runs()).unwrap()
    );
}
