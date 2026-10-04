mod common;
use common::*;
use fallout_runtime::{
    Error, Limits, World,
    events::{Context, Trigger},
    identity::{CampaignId, Owner, ReferenceValue, Value},
    inventory::{Ammo, Facts, OpaqueExtra, Ownership},
    snapshot::Snapshot,
};
use serde_json::{Value as Json, json};

fn fixture() -> (tempfile::TempDir, fallout_data::loaded_scripts::Catalogue) {
    let dir = tempfile::tempdir().unwrap();
    write_fixture(dir.path(), false);
    let catalogue = load(dir.path(), &["FalloutNV.esm"]);
    (dir, catalogue)
}

fn world(catalogue: &fallout_data::loaded_scripts::Catalogue) -> World<'_> {
    let mut world = World::with_campaign(
        catalogue,
        Limits::default(),
        CampaignId::from_bytes([0x51; 16]).unwrap(),
    )
    .unwrap();
    let reference = world.register_reference(Some(form(0x100))).unwrap();
    let context = Context {
        arguments: vec![ReferenceValue::Null, ReferenceValue::Live { id: reference }],
        ..Context::default()
    };
    let handle = world
        .create_instance(
            &definition(catalogue),
            Owner::Placed { reference },
            context.clone(),
        )
        .unwrap();
    world
        .assign(
            handle,
            &[(
                42,
                Value::Number {
                    bits: 0x7ff8_1234_5678_9abc,
                },
            )],
        )
        .unwrap();
    world
        .enqueue(handle, Trigger::ObjectEvent { mask: 0x80 }, context)
        .unwrap();
    world.initialize_inventory(reference).unwrap();
    let mut facts = Facts::unknown(form(0x100));
    facts.ownership = Some(Ownership::Unowned);
    facts.equipped_slots = Some(vec![7, 2]);
    facts.ammo = Some(Ammo {
        base: form(0x100),
        count: 1,
    });
    facts.modifications = Some(vec![form(0x100)]);
    facts.script_instance = Some(world.instance(handle).unwrap().id());
    facts.extra_fields = vec![
        OpaqueExtra {
            tag: *b"ONE!",
            bytes: vec![0, 255],
        },
        OpaqueExtra {
            tag: *b"TWO!",
            bytes: vec![1],
        },
    ];
    world
        .add_item(reference, facts, 2.try_into().unwrap())
        .unwrap();
    world
}

fn decode(value: &Json, limits: Limits, schema: u32) -> fallout_runtime::Result<Snapshot> {
    let bytes = serde_json::to_vec(value).unwrap();
    decode_bytes(&bytes, limits, schema)
}
fn decode_bytes(bytes: &[u8], limits: Limits, schema: u32) -> fallout_runtime::Result<Snapshot> {
    match schema {
        1 => Snapshot::migrate_v1(bytes, limits, CampaignId::from_bytes([0x51; 16]).unwrap()),
        2 => Snapshot::migrate_v2(bytes, limits),
        3 => Snapshot::migrate_v3(bytes, limits),
        4 => Snapshot::decode(bytes, limits),
        _ => unreachable!(),
    }
}
fn capacity(result: fallout_runtime::Result<Snapshot>, expected: &'static str) {
    assert!(
        matches!(&result, Err(Error::Capacity(label)) if *label == expected),
        "{result:?}"
    );
}
fn legacy(mut value: Json, schema: u32) -> Json {
    let map = value.as_object_mut().unwrap();
    map.insert("schema_version".into(), schema.into());
    if schema < 3 {
        map.remove("next_item");
        map.remove("inventory_banks");
    }
    map.remove("reference_states");
    if schema == 1 {
        map.remove("campaign");
        map.remove("state_revision");
    }
    value
}
fn positional(value: &mut Json, fields: &[&str]) {
    let mut map = value.take().as_object().unwrap().clone();
    *value = Json::Array(fields.iter().map(|key| map.remove(*key).unwrap()).collect());
    assert!(map.is_empty());
}
fn positional_root(value: &mut Json, schema: u32) {
    let fields: &[&str] = match schema {
        1 => &[
            "schema_version",
            "profile",
            "catalogue_sha256",
            "next_instance",
            "next_reference",
            "next_event_sequence",
            "clocks",
            "references",
            "instances",
            "pending_events",
        ],
        2 => &[
            "schema_version",
            "campaign",
            "state_revision",
            "profile",
            "catalogue_sha256",
            "next_instance",
            "next_reference",
            "next_event_sequence",
            "clocks",
            "references",
            "instances",
            "pending_events",
        ],
        3 | 4 => &[
            "schema_version",
            "campaign",
            "state_revision",
            "profile",
            "catalogue_sha256",
            "next_item",
            "inventory_banks",
            "next_instance",
            "next_reference",
            "next_event_sequence",
            "clocks",
            "references",
            "instances",
            "pending_events",
            "reference_states",
        ],
        _ => unreachable!(),
    };
    positional(
        value,
        if schema == 3 {
            &fields[..fields.len() - 1]
        } else {
            fields
        },
    );
}
fn positional_nested(value: &mut Json, schema: u32) {
    for instance in value["instances"].as_array_mut().unwrap() {
        positional(
            &mut instance["context"],
            &[
                "calling_reference",
                "containing_reference",
                "target",
                "arguments",
            ],
        );
        for local in instance["locals"].as_array_mut().unwrap() {
            positional(local, &["index", "value"]);
        }
        positional(
            instance,
            &["id", "definition", "owner", "context", "locals"],
        );
    }
    for event in value["pending_events"].as_array_mut().unwrap() {
        positional(
            &mut event["context"],
            &[
                "calling_reference",
                "containing_reference",
                "target",
                "arguments",
            ],
        );
        positional(
            &mut event["arrived"],
            &[
                "tick",
                "game_nanoseconds",
                "menu_nanoseconds",
                "real_nanoseconds",
            ],
        );
        positional(
            event,
            &["sequence", "instance", "trigger", "context", "arrived"],
        );
    }
    for reference in value["references"].as_array_mut().unwrap() {
        positional(reference, &["id", "authored"]);
    }
    if schema >= 3 {
        for bank in value["inventory_banks"].as_array_mut().unwrap() {
            for item in bank["items"].as_array_mut().unwrap() {
                for extra in item["facts"]["extra_fields"].as_array_mut().unwrap() {
                    positional(extra, &["tag", "bytes"]);
                }
                positional(&mut item["facts"]["ammo"], &["base", "count"]);
                positional(
                    &mut item["facts"],
                    &[
                        "base",
                        "condition",
                        "ownership",
                        "equipped_slots",
                        "ammo",
                        "modifications",
                        "quest_item",
                        "script_instance",
                        "extra_fields",
                    ],
                );
                positional(item, &["id", "owner", "count", "facts"]);
            }
            positional(bank, &["owner", "items"]);
        }
    }
}

#[test]
fn current_and_legacy_top_level_limits_precede_owned_element_validation() {
    // Deliberately invalid elements distinguish admission from the old decoder,
    // which would reject the first null before reaching any collection budget.
    for schema in 1..=4 {
        for (field, limits, label) in [
            (
                "references",
                Limits {
                    max_references: 1,
                    ..Limits::default()
                },
                "saved references",
            ),
            (
                "instances",
                Limits {
                    max_instances: 1,
                    ..Limits::default()
                },
                "saved instances",
            ),
            (
                "pending_events",
                Limits {
                    max_pending_events: 1,
                    ..Limits::default()
                },
                "saved events",
            ),
        ] {
            let value = json!({field: [null, null]});
            capacity(decode(&value, limits, schema), label);
            let escaped = serde_json::to_string(&value).unwrap().replace(
                field,
                &format!("\\u{:04x}{}", field.as_bytes()[0], &field[1..]),
            );
            capacity(decode_bytes(escaped.as_bytes(), limits, schema), label);
        }
    }
    capacity(
        decode(
            &json!({"inventory_banks": [null, null]}),
            Limits {
                max_inventory_banks: 1,
                ..Limits::default()
            },
            4,
        ),
        "saved inventory banks",
    );
}

#[test]
fn locals_and_items_are_aggregated_across_instances_and_banks() {
    for schema in 1..=4 {
        capacity(
            decode(
                &json!({"instances": [{"locals": [null]}, {"locals": [null]}]}),
                Limits {
                    max_locals: 1,
                    ..Limits::default()
                },
                schema,
            ),
            "saved locals",
        );
    }
    capacity(
        decode(
            &json!({"inventory_banks": [{"items": [null]}, {"items": [null]}]}),
            Limits {
                max_item_instances: 1,
                ..Limits::default()
            },
            4,
        ),
        "saved items",
    );
}

#[test]
fn instance_and_event_context_arguments_are_bounded_independently() {
    for schema in 1..=4 {
        for (field, label) in [
            ("instances", "saved instance context arguments"),
            ("pending_events", "saved event context arguments"),
        ] {
            capacity(
                decode(
                    &json!({field: [{"context": {"arguments": [null, null]}}]}),
                    Limits {
                        max_event_arguments: 1,
                        ..Limits::default()
                    },
                    schema,
                ),
                label,
            );
        }
    }
}

#[test]
fn item_link_budget_includes_all_vectors_and_optional_links() {
    for facts in [
        json!({"equipped_slots": [null, null]}),
        json!({"modifications": [null, null]}),
        json!({"extra_fields": [null, null]}),
        json!({"equipped_slots": [null], "ownership": {"kind": "unowned"}}),
        json!({"ammo": {"base": null, "count": 0}, "script_instance": 1}),
    ] {
        capacity(
            decode(
                &json!({"inventory_banks": [{"items": [{"facts": facts}]}]}),
                Limits {
                    max_item_links: 1,
                    ..Limits::default()
                },
                4,
            ),
            "saved item extra state",
        );
    }
    capacity(
        decode(
            &json!({"inventory_banks": [{"items": [{"facts": {"modifications": [null]}}, {"facts": {"extra_fields": [null]}}]}]}),
            Limits {
                max_total_item_links: 1,
                ..Limits::default()
            },
            4,
        ),
        "saved item extra state",
    );
}

#[test]
fn opaque_bytes_are_aggregated_per_item_and_across_banks() {
    let first = json!({"facts": {"extra_fields": [{"bytes": [null]}, {"bytes": [null]}]}});
    capacity(
        decode(
            &json!({"inventory_banks": [{"items": [first.clone()]}]}),
            Limits {
                max_item_bytes: 1,
                ..Limits::default()
            },
            4,
        ),
        "saved item extra state",
    );
    capacity(
        decode(
            &json!({"inventory_banks": [{"items": [first.clone()]}, {"items": [first]}]}),
            Limits {
                max_total_item_bytes: 3,
                ..Limits::default()
            },
            4,
        ),
        "saved item extra state",
    );
}

#[test]
fn zero_and_byte_budgets_reject_before_owned_admission() {
    capacity(
        Snapshot::decode(
            b"{",
            Limits {
                max_snapshot_bytes: 0,
                ..Limits::default()
            },
        ),
        "snapshot bytes",
    );
    for schema in 1..=2 {
        capacity(
            decode_bytes(
                b"{",
                Limits {
                    max_snapshot_bytes: 0,
                    ..Limits::default()
                },
                schema,
            ),
            "legacy snapshot bytes",
        );
    }
    capacity(
        Snapshot::decode(
            b"{\"references\":[null]}",
            Limits {
                max_references: 0,
                ..Limits::default()
            },
        ),
        "saved references",
    );
}

#[test]
fn positional_current_and_legacy_structs_preserve_values_and_restoration() {
    let (_dir, catalogue) = fixture();
    let current = world(&catalogue).snapshot();
    for schema in 1..=4 {
        let expected = if schema >= 3 {
            current.clone()
        } else {
            let mut expected = current.clone();
            expected.next_item = 1;
            expected.inventory_banks.clear();
            if schema == 1 {
                expected.state_revision = 0;
            }
            expected
        };
        let value = serde_json::to_value(&current).unwrap();
        let value = if schema == 4 {
            value
        } else {
            legacy(value, schema)
        };
        for root_array in [false, true] {
            for nested_array in [false, true] {
                let mut candidate = value.clone();
                if nested_array {
                    positional_nested(&mut candidate, schema);
                }
                if root_array {
                    positional_root(&mut candidate, schema);
                }
                let snapshot = decode(&candidate, Limits::default(), schema).unwrap();
                assert_eq!(snapshot, expected);
                assert_eq!(
                    World::restore(&catalogue, snapshot, Limits::default())
                        .unwrap()
                        .snapshot(),
                    expected
                );
            }
        }
    }
}

#[test]
fn positional_collections_obey_each_existing_limit() {
    let (_dir, catalogue) = fixture();
    let current = serde_json::to_value(world(&catalogue).snapshot()).unwrap();
    for schema in 1..=4 {
        let mut value = if schema == 4 {
            current.clone()
        } else {
            legacy(current.clone(), schema)
        };
        positional_nested(&mut value, schema);
        positional_root(&mut value, schema);
        for (limits, label) in [
            (
                Limits {
                    max_references: 0,
                    ..Limits::default()
                },
                "saved references",
            ),
            (
                Limits {
                    max_instances: 0,
                    ..Limits::default()
                },
                "saved instances",
            ),
            (
                Limits {
                    max_locals: 2,
                    ..Limits::default()
                },
                "saved locals",
            ),
            (
                Limits {
                    max_pending_events: 0,
                    ..Limits::default()
                },
                "saved events",
            ),
            (
                Limits {
                    max_event_arguments: 1,
                    ..Limits::default()
                },
                "saved instance context arguments",
            ),
        ] {
            capacity(decode(&value, limits, schema), label);
        }
        if schema >= 3 {
            for limits in [
                Limits {
                    max_inventory_banks: 0,
                    ..Limits::default()
                },
                Limits {
                    max_item_instances: 0,
                    ..Limits::default()
                },
                Limits {
                    max_item_links: 7,
                    ..Limits::default()
                },
                Limits {
                    max_total_item_links: 7,
                    ..Limits::default()
                },
                Limits {
                    max_item_bytes: 2,
                    ..Limits::default()
                },
                Limits {
                    max_total_item_bytes: 2,
                    ..Limits::default()
                },
            ] {
                let label = if limits.max_inventory_banks == 0 {
                    "saved inventory banks"
                } else if limits.max_item_instances == 0 {
                    "saved items"
                } else {
                    "saved item extra state"
                };
                capacity(decode(&value, limits, schema), label);
            }
        }
    }
}

#[test]
fn exact_boundaries_escaped_names_and_optional_nulls_remain_accepted() {
    let (_dir, catalogue) = fixture();
    let snapshot = world(&catalogue).snapshot();
    let limits = Limits {
        max_references: 1,
        max_instances: 1,
        max_locals: 3,
        max_pending_events: 1,
        max_event_arguments: 2,
        max_inventory_banks: 1,
        max_item_instances: 1,
        max_item_links: 8,
        max_total_item_links: 8,
        max_item_bytes: 3,
        max_total_item_bytes: 3,
        ..Limits::default()
    };
    let bytes = serde_json::to_string(&snapshot)
        .unwrap()
        .replace("\"references\"", "\"ref\\u0065rences\"")
        .replace("\"locals\"", "\"loc\\u0061ls\"")
        .replace("\"arguments\"", "\"arg\\u0075ments\"")
        .replace("\"inventory_banks\"", "\"inventory_\\u0062anks\"")
        .replace("\"items\"", "\"it\\u0065ms\"")
        .replace("\"facts\"", "\"f\\u0061cts\"")
        .replace("\"extra_fields\"", "\"extra_\\u0066ields\"")
        .replace("\"bytes\"", "\"by\\u0074es\"");
    assert_eq!(
        Snapshot::decode(bytes.as_bytes(), limits).unwrap(),
        snapshot
    );
    // Escaping a bounded nested field must not bypass admission either.
    for reduced in [
        Limits {
            max_locals: 2,
            ..limits
        },
        Limits {
            max_event_arguments: 1,
            ..limits
        },
        Limits {
            max_item_instances: 0,
            ..limits
        },
        Limits {
            max_item_bytes: 2,
            ..limits
        },
    ] {
        assert!(matches!(
            Snapshot::decode(bytes.as_bytes(), reduced),
            Err(Error::Capacity(_))
        ));
    }
    let mut null_facts = world(&catalogue);
    let id = null_facts.snapshot().inventory_banks[0].items[0].id();
    null_facts
        .replace_item_facts(id, Facts::unknown(form(0x100)))
        .unwrap();
    let empty = null_facts.snapshot();
    assert!(
        Snapshot::decode(
            &empty.encode(1 << 20).unwrap(),
            Limits {
                max_item_links: 0,
                max_total_item_links: 0,
                max_item_bytes: 0,
                max_total_item_bytes: 0,
                ..limits
            }
        )
        .is_ok()
    );

    // Per-item counters reset, while aggregate counters span banks/items.
    let mut repeated = serde_json::to_value(&snapshot).unwrap();
    let item = repeated["inventory_banks"][0]["items"][0].clone();
    repeated["inventory_banks"][0]["items"]
        .as_array_mut()
        .unwrap()
        .push(item);
    let doubled = Limits {
        max_item_instances: 2,
        max_total_item_links: 16,
        max_total_item_bytes: 6,
        ..limits
    };
    assert_eq!(
        decode(&repeated, doubled, 4).unwrap().inventory_banks[0]
            .items
            .len(),
        2
    );
    capacity(
        decode(
            &repeated,
            Limits {
                max_total_item_links: 15,
                ..doubled
            },
            4,
        ),
        "saved item extra state",
    );
}

#[test]
fn strict_owned_validation_still_rejects_unknown_duplicate_and_malformed_input() {
    let (_dir, catalogue) = fixture();
    let current = serde_json::to_value(world(&catalogue).snapshot()).unwrap();
    for schema in 1..=4 {
        let value = if schema == 4 {
            current.clone()
        } else {
            legacy(current.clone(), schema)
        };
        let mut unknown = value.clone();
        unknown["unknown"] = true.into();
        assert!(matches!(
            decode(&unknown, Limits::default(), schema),
            Err(Error::Json(_))
        ));
        let body = serde_json::to_string(&value).unwrap();
        let duplicate = format!("{{\"schema_version\":{schema},{}", &body[1..]);
        let escaped_duplicate = format!("{{\"schema_\\u0076ersion\":{schema},{}", &body[1..]);
        for malformed in [
            duplicate.as_bytes(),
            escaped_duplicate.as_bytes(),
            &body.as_bytes()[..body.len() - 1],
            b"{} true",
            b"{\"references\":[}",
        ] {
            assert!(matches!(
                decode_bytes(malformed, Limits::default(), schema),
                Err(Error::Json(_))
            ));
        }
        let mut extra_element = value.clone();
        positional_root(&mut extra_element, schema);
        extra_element.as_array_mut().unwrap().push(Json::Null);
        assert!(matches!(
            decode(&extra_element, Limits::default(), schema),
            Err(Error::Json(_))
        ));
    }
}

#[test]
fn deeply_nested_skipped_and_counted_values_fail_with_a_bounded_error() {
    for schema in 1..=4 {
        for prefix in [
            "{\"clocks\":",
            "{\"instances\":[{\"locals\":[",
            "{\"unknown\":",
        ] {
            let suffix = if prefix.contains("locals") {
                "]}]}"
            } else {
                "}"
            };
            let input = format!(
                "{prefix}{}0{}{suffix}",
                "[".repeat(10_000),
                "]".repeat(10_000)
            );
            let result = decode_bytes(input.as_bytes(), Limits::default(), schema);
            let Err(Error::Json(error)) = result else {
                panic!("{result:?}");
            };
            assert!(
                error.to_string().contains("recursion limit exceeded")
                    || error.to_string().contains("nesting limit exceeded"),
                "{error}"
            );
        }
    }
}
