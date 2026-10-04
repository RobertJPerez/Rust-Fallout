//! Bounded display of an already classified comparison. Never execution authority.
use crate::Result;
use fallout_data::identity::FormKey;
use fallout_runtime::execution::trace::{
    self, Caller, Capture, Identity, Manifest, Producer, StepInput, Word,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const TEXT_PREFIX_BYTES: usize = 64;
const MAXIMUM_COMPACT_BYTES: usize = 8192;

fn text(value: &str) -> Value {
    if value.len() <= TEXT_PREFIX_BYTES {
        return json!(value);
    }
    let mut end = TEXT_PREFIX_BYTES;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    json!({"prefix":&value[..end],"utf8_bytes":value.len(),
        "sha256":format!("{:x}",Sha256::digest(value.as_bytes())),"truncated":true})
}
fn form(value: Option<&FormKey>) -> Value {
    value.map_or(Value::Null, |value| {
        json!({"profile":value.profile,
        "origin_plugin":text(&value.origin_plugin),"local_id":value.local_id})
    })
}
fn caller(value: &Caller) -> Value {
    json!({"activation":value.activation,"calling_reference":form(value.calling_reference.as_ref()),
        "containing_reference":form(value.containing_reference.as_ref()),"target":form(value.target.as_ref())})
}
fn word(value: Option<&Word>) -> Value {
    value.map_or(
        Value::Null,
        |value| json!({"format":value.format,"bits":text(&value.bits)}),
    )
}
fn write(value: Option<&trace::LocalWrite>) -> Value {
    value.map_or(
        Value::Null,
        |value| json!({"index":value.index,"value":word(Some(&value.value))}),
    )
}
fn site(value: Option<&StepInput>, expected: Option<&StepInput>) -> Value {
    value.map_or(Value::Null,|value|json!({"event_ordinal":value.event_ordinal,"event_id":value.event_id,
        "begin_scda_offset":value.begin_scda_offset,"scda_offset":value.scda_offset,
        "operation":value.operation,"caller":caller(&value.caller),"operand_count":value.operands.len(),
        "item":form(value.item.as_ref()),"matches_manifest_input":expected.is_some_and(|expected|expected==value)}))
}
fn pair(path: String, expected_side: &'static str, expected: Value, observed: Value) -> Value {
    json!({"path":path,"expected_side":expected_side,"expected":expected,"observed":observed})
}
fn position<T: PartialEq>(expected: &[T], observed: &[T]) -> usize {
    expected
        .iter()
        .zip(observed)
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| expected.len().min(observed.len()))
}
fn input(expected: &StepInput, observed: &StepInput) -> Value {
    macro_rules! scalar {
        ($field:ident) => {
            if expected.$field != observed.$field {
                return pair(
                    concat!("/input/", stringify!($field)).into(),
                    "manifest",
                    json!(expected.$field),
                    json!(observed.$field),
                );
            }
        };
    }
    scalar!(event_ordinal);
    scalar!(event_id);
    scalar!(begin_scda_offset);
    scalar!(scda_offset);
    scalar!(operation);
    for (name, left, right) in [
        (
            "calling_reference",
            &expected.caller.calling_reference,
            &observed.caller.calling_reference,
        ),
        (
            "containing_reference",
            &expected.caller.containing_reference,
            &observed.caller.containing_reference,
        ),
        ("target", &expected.caller.target, &observed.caller.target),
    ] {
        if left != right {
            return pair(
                format!("/input/caller/{name}"),
                "manifest",
                form(left.as_ref()),
                form(right.as_ref()),
            );
        }
    }
    if expected.caller.activation != observed.caller.activation {
        return pair(
            "/input/caller/activation".into(),
            "manifest",
            json!(expected.caller.activation),
            json!(observed.caller.activation),
        );
    }
    if expected.operands != observed.operands {
        let index = position(&expected.operands, &observed.operands);
        return pair(
            format!("/input/operands/{index}"),
            "manifest",
            word(expected.operands.get(index)),
            word(observed.operands.get(index)),
        );
    }
    pair(
        "/input/item".into(),
        "manifest",
        form(expected.item.as_ref()),
        form(observed.item.as_ref()),
    )
}
fn identity(expected: &Identity, observed: &Identity) -> Value {
    for (name, left, right) in [
        (
            "executable_sha256",
            &expected.executable_sha256,
            &observed.executable_sha256,
        ),
        (
            "profile_receipt_sha256",
            &expected.profile_receipt_sha256,
            &observed.profile_receipt_sha256,
        ),
        (
            "source_cohort_sha256",
            &expected.source_cohort_sha256,
            &observed.source_cohort_sha256,
        ),
        (
            "winning_content_sha256",
            &expected.winning_content_sha256,
            &observed.winning_content_sha256,
        ),
    ] {
        if left != right {
            return pair(
                format!("/identity/{name}"),
                "manifest",
                text(left),
                text(right),
            );
        }
    }
    if expected.definition.key.record != observed.definition.key.record {
        return pair(
            "/identity/definition/key/record".into(),
            "manifest",
            form(Some(&expected.definition.key.record)),
            form(Some(&observed.definition.key.record)),
        );
    }
    if expected.definition.key.header_decoded_offset
        != observed.definition.key.header_decoded_offset
    {
        return pair(
            "/identity/definition/key/header_decoded_offset".into(),
            "manifest",
            json!(expected.definition.key.header_decoded_offset),
            json!(observed.definition.key.header_decoded_offset),
        );
    }
    for (name, left, right) in [
        (
            "/identity/definition/version_sha256",
            &expected.definition.version_sha256,
            &observed.definition.version_sha256,
        ),
        (
            "/identity/compiled_sha256",
            &expected.compiled_sha256,
            &observed.compiled_sha256,
        ),
    ] {
        if left != right {
            return pair(name.into(), "manifest", text(left), text(right));
        }
    }
    pair(
        "/identity/compiled_bytes".into(),
        "manifest",
        json!(expected.compiled_bytes),
        json!(observed.compiled_bytes),
    )
}

/// Called only after the existing comparator has validated every supplied input.
/// Select at most one row/value pair; never serialize a captured operand/effect list.
pub(super) fn describe(
    manifest: &Manifest,
    original: Option<&Capture>,
    replacement: Option<&Capture>,
    comparison: &trace::Comparison,
) -> Result<Value> {
    let Some(difference) = &comparison.first_difference else {
        return Ok(Value::Null);
    };
    let selected = match difference.producer {
        Producer::Original => original,
        Producer::Replacement => replacement,
    };
    let index = difference.step.or_else(|| {
        if difference.field == "step_count" {
            selected.map(|capture| capture.steps.len().min(manifest.steps.len()))
        } else {
            None
        }
    });
    let expected_input = index.and_then(|index| manifest.steps.get(index));
    let selected_step =
        index.and_then(|index| selected.and_then(|capture| capture.steps.get(index)));
    let original_step =
        index.and_then(|index| original.and_then(|capture| capture.steps.get(index)));
    let replacement_step =
        index.and_then(|index| replacement.and_then(|capture| capture.steps.get(index)));
    let value = match difference.field {
        "missing_capture" => pair(
            "/capture".into(),
            "manifest",
            json!({"required_producer":difference.producer}),
            Value::Null,
        ),
        "capture_identity" => {
            let capture = selected.ok_or("classified identity difference lacks capture")?;
            if capture.producer != difference.producer {
                pair(
                    "/producer".into(),
                    "manifest",
                    json!(difference.producer),
                    json!(capture.producer),
                )
            } else {
                identity(&manifest.identity, &capture.identity)
            }
        }
        "producer_executable" => {
            let expected = if difference.producer == Producer::Original {
                text(&manifest.identity.executable_sha256)
            } else {
                json!({"relation":"different_from_original","original_executable_sha256":text(&manifest.identity.executable_sha256)})
            };
            pair(
                "/producer_executable_sha256".into(),
                "manifest",
                expected,
                text(
                    &selected
                        .ok_or("producer capture missing")?
                        .producer_executable_sha256,
                ),
            )
        }
        "incomplete_capture" => pair(
            "/finish".into(),
            "manifest",
            json!(trace::Finish::Completed),
            json!(selected.ok_or("incomplete capture missing")?.finish),
        ),
        "step_count" => pair(
            format!("/steps/{}", index.ok_or("step count index missing")?),
            "manifest",
            site(expected_input, expected_input),
            site(selected_step.map(|step| &step.input), expected_input),
        ),
        "step_input" => input(
            expected_input.ok_or("manifest input missing")?,
            &selected_step.ok_or("observed input missing")?.input,
        ),
        "missing_observation" => {
            let step = selected_step.ok_or("observation step missing")?;
            let required = match step.input.operation {
                trace::Operation::Assignment | trace::Operation::Conversion => {
                    "local_writes_or_error"
                }
                trace::Operation::Branch => "successor_or_error",
                trace::Operation::GetItemCount => "return_value_or_error",
            };
            pair(
                "/output".into(),
                "manifest",
                json!({"required":required}),
                json!({
                "return_value_present":step.output.return_value.is_some(),"write_count":step.output.writes.len(),
                "successor_scda_offset":step.output.successor_scda_offset,"error_present":step.output.error.is_some()}),
            )
        }
        field => {
            let left = &original_step.ok_or("original output step missing")?.output;
            let right = &replacement_step
                .ok_or("replacement output step missing")?
                .output;
            match field {
                "return_value" => pair(
                    "/output/return_value".into(),
                    "original",
                    word(left.return_value.as_ref()),
                    word(right.return_value.as_ref()),
                ),
                "successor_scda_offset" => pair(
                    "/output/successor_scda_offset".into(),
                    "original",
                    json!(left.successor_scda_offset),
                    json!(right.successor_scda_offset),
                ),
                "local_writes" => {
                    let index = position(&left.writes, &right.writes);
                    pair(
                        format!("/output/writes/{index}"),
                        "original",
                        write(left.writes.get(index)),
                        write(right.writes.get(index)),
                    )
                }
                "error" => {
                    let mut value = pair(
                        "/output/error".into(),
                        "original",
                        left.error.as_deref().map_or(Value::Null, text),
                        right.error.as_deref().map_or(Value::Null, text),
                    );
                    let first = left
                        .error
                        .as_deref()
                        .zip(right.error.as_deref())
                        .map(|(left, right)| position(left.as_bytes(), right.as_bytes()));
                    value["first_differing_utf8_byte"] = json!(first);
                    value
                }
                _ => return Err("unknown classified trace difference".into()),
            }
        }
    };
    let context = json!({"schema_version":1,"diagnostic_only":true,"producer":difference.producer,
        "field":difference.field,"step":index,"counts":{"manifest":manifest.steps.len(),
        "original":original.map(|capture|capture.steps.len()),"replacement":replacement.map(|capture|capture.steps.len())},
        "manifest_site":site(expected_input,expected_input),
        "original_site":site(original_step.map(|step|&step.input),expected_input),
        "replacement_site":site(replacement_step.map(|step|&step.input),expected_input),"difference":value,
        "maximum_compact_bytes":MAXIMUM_COMPACT_BYTES});
    if serde_json::to_vec(&context)?.len() > MAXIMUM_COMPACT_BYTES {
        return Err("trace difference display budget exceeded".into());
    }
    Ok(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallout_data::{
        identity::ProfileId,
        loaded_scripts::{Handle, ScriptKey},
    };
    fn fixture() -> (Manifest, Capture, Capture) {
        let input = StepInput {
            event_ordinal: 0,
            event_id: 0,
            begin_scda_offset: 0,
            scda_offset: 10,
            operation: trace::Operation::Assignment,
            caller: Caller {
                calling_reference: None,
                containing_reference: None,
                target: None,
                activation: 1,
            },
            operands: vec![Word::binary64(0x8000000000000000)],
            item: None,
        };
        let manifest = Manifest {
            schema_version: 1,
            purpose: trace::Operation::Assignment,
            steps: vec![input.clone()],
            identity: Identity {
                executable_sha256: "a".repeat(64),
                profile_receipt_sha256: "b".repeat(64),
                source_cohort_sha256: "c".repeat(64),
                winning_content_sha256: "d".repeat(64),
                compiled_sha256: "e".repeat(64),
                compiled_bytes: 26,
                definition: Handle {
                    key: ScriptKey {
                        record: FormKey {
                            profile: ProfileId::NvOriginal,
                            origin_plugin: "authored.esm".into(),
                            local_id: 0x300,
                        },
                        header_decoded_offset: 0,
                    },
                    version_sha256: "f".repeat(64),
                },
            },
        };
        let original = Capture {
            schema_version: 1,
            identity: manifest.identity.clone(),
            producer: Producer::Original,
            producer_executable_sha256: "a".repeat(64),
            transport_receipt_sha256: "0".repeat(64),
            instrumentation: "Synthetic display-only test, never original execution".into(),
            finish: trace::Finish::Completed,
            steps: vec![trace::Step {
                input,
                output: trace::StepOutput {
                    return_value: None,
                    successor_scda_offset: Some(22),
                    writes: vec![trace::LocalWrite {
                        index: 2,
                        value: Word::binary64(0x8000000000000000),
                    }],
                    error: None,
                },
            }],
        };
        let mut replacement = original.clone();
        replacement.producer = Producer::Replacement;
        replacement.producer_executable_sha256 = "1".repeat(64);
        (manifest, original, replacement)
    }
    fn comparison(field: &'static str, step: Option<usize>) -> trace::Comparison {
        trace::Comparison {
            status: trace::Status::Mismatched,
            compared_steps: 0,
            first_difference: Some(trace::Difference {
                producer: Producer::Replacement,
                step,
                field,
            }),
            gameplay_accepted: false,
        }
    }
    #[test]
    fn late_operand_difference_retains_only_one_exact_word_and_source_site() {
        let (mut manifest, mut original, mut replacement) = fixture();
        manifest.steps[0].operands = vec![Word::binary64(0); 4096];
        original.steps[0].input = manifest.steps[0].clone();
        replacement.steps[0].input = manifest.steps[0].clone();
        replacement.steps[0].input.operands[4095] = Word::binary64(0x7ff8123456789abc);
        let value = describe(
            &manifest,
            Some(&original),
            Some(&replacement),
            &comparison("step_input", Some(0)),
        )
        .unwrap();
        assert_eq!(value["difference"]["path"], "/input/operands/4095");
        assert_eq!(value["difference"]["expected"]["bits"], "0000000000000000");
        assert_eq!(value["difference"]["observed"]["bits"], "7ff8123456789abc");
        assert_eq!(value["replacement_site"]["matches_manifest_input"], false);
        assert!(serde_json::to_vec(&value).unwrap().len() < 2000);
    }
    #[test]
    fn long_form_names_and_utf8_errors_are_bounded_before_display_retention() {
        let (mut manifest, mut original, mut replacement) = fixture();
        let form = FormKey {
            profile: ProfileId::NvOriginal,
            origin_plugin: "a\"".repeat(131_072) + ".esm",
            local_id: 0x200,
        };
        for role in [
            &mut manifest.steps[0].caller,
            &mut original.steps[0].input.caller,
            &mut replacement.steps[0].input.caller,
        ] {
            role.calling_reference = Some(form.clone());
            role.containing_reference = Some(form.clone());
            role.target = Some(form.clone());
        }
        original.steps[0].output.error = Some("\u{1}".repeat(4096));
        replacement.steps[0].output.error = Some("\u{1}".repeat(4095) + "\u{2}");
        let value = describe(
            &manifest,
            Some(&original),
            Some(&replacement),
            &comparison("error", Some(0)),
        )
        .unwrap();
        assert_eq!(value["difference"]["first_differing_utf8_byte"], 4095);
        assert_eq!(value["difference"]["expected"]["utf8_bytes"], 4096);
        assert_eq!(
            value["manifest_site"]["caller"]["target"]["origin_plugin"]["utf8_bytes"],
            form.origin_plugin.len()
        );
        assert!(serde_json::to_vec(&value).unwrap().len() <= MAXIMUM_COMPACT_BYTES);
        let unicode = text(&"☃é".repeat(100));
        let prefix = unicode["prefix"].as_str().unwrap();
        assert!(prefix.len() <= TEXT_PREFIX_BYTES && prefix.len() > TEXT_PREFIX_BYTES - 3);
        assert_eq!(
            unicode["sha256"],
            format!("{:x}", Sha256::digest("☃é".repeat(100).as_bytes()))
        );
    }
    #[test]
    fn missing_extra_and_global_identity_differences_do_not_invent_source_steps() {
        let (manifest, original, mut replacement) = fixture();
        replacement.steps.clear();
        let value = describe(
            &manifest,
            Some(&original),
            Some(&replacement),
            &comparison("step_count", None),
        )
        .unwrap();
        assert_eq!(value["step"], 0);
        assert_eq!(value["replacement_site"], Value::Null);
        assert_eq!(value["manifest_site"]["scda_offset"], 10);
        replacement.steps = vec![original.steps[0].clone(); 2];
        let value = describe(
            &manifest,
            Some(&original),
            Some(&replacement),
            &comparison("step_count", None),
        )
        .unwrap();
        assert_eq!(value["step"], 1);
        assert_eq!(value["manifest_site"], Value::Null);
        assert_eq!(value["replacement_site"]["scda_offset"], 10);
        replacement.identity.profile_receipt_sha256 = "9".repeat(64);
        let value = describe(
            &manifest,
            Some(&original),
            Some(&replacement),
            &comparison("capture_identity", None),
        )
        .unwrap();
        assert_eq!(
            value["difference"]["path"],
            "/identity/profile_receipt_sha256"
        );
        assert_eq!(value["step"], Value::Null);
        assert_eq!(value["replacement_site"], Value::Null);
        let matched = trace::Comparison {
            status: trace::Status::Matched,
            compared_steps: 1,
            first_difference: None,
            gameplay_accepted: false,
        };
        assert_eq!(
            describe(&manifest, Some(&original), Some(&original), &matched).unwrap(),
            Value::Null
        );
    }
}
