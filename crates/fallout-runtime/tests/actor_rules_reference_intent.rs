#[path = "actor_rules_reference_intent_fixture.rs"]
mod fixture;
use fallout_runtime::{
    Limits as WorldLimits,
    actor_rules::reference_intent::{self, Error, Intent, Limits},
    snapshot::Snapshot,
};
use fixture::*;

#[test]
fn explicit_npc_creature_initialize_replace_and_cold_continue_preserve_all_other_state() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, content, sources| {
        let live = world(scripts);
        let original = live.snapshot();
        for (id, base) in [(1, 0x100), (2, 0x200), (3, 0x100)] {
            let mut request = choice(&original, id, base);
            for scale in [None, Some(0.75f32.to_bits())] {
                request.scale_bits = scale;
                let candidate = reference_intent::apply_private(
                    &original,
                    scripts,
                    content,
                    sources,
                    &request,
                    WorldLimits::default(),
                    Limits::default(),
                )
                .unwrap();
                let state = candidate
                    .snapshot()
                    .reference_states
                    .iter()
                    .find(|s| s.id == reference(id))
                    .unwrap();
                let json = serde_json::to_value(&state.state).unwrap();
                assert_eq!(
                    json["pose"]["position_bits"],
                    serde_json::json!(request.position_bits)
                );
                assert_eq!(
                    json["pose"]["rotation_bits"],
                    serde_json::json!(request.rotation_bits)
                );
                assert_eq!(json["pose"]["scale_bits"], serde_json::json!(scale));
                assert_eq!(json["enabled"], false);
                assert_eq!(
                    candidate.actor_before().reference.state().is_some(),
                    id == 3
                );
                assert_eq!(candidate.actor_before().actor.key, &form(base));
                conserve(&original, candidate.snapshot(), true, false, 1);
                assert_cold(scripts, candidate.snapshot());
                assert_eq!(live.snapshot(), original);
                let again = choice(candidate.snapshot(), id, base);
                let next = reference_intent::apply_private(
                    candidate.snapshot(),
                    scripts,
                    content,
                    sources,
                    &again,
                    WorldLimits::default(),
                    Limits::default(),
                )
                .unwrap();
                conserve(candidate.snapshot(), next.snapshot(), true, false, 1);
            }
        }
    });
}
#[test]
fn wrong_claim_source_reference_cell_pose_scale_and_faithful_never_return_candidate() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, content, sources| {
        let live = world(scripts);
        let before = live.snapshot();
        let mut requests = Vec::new();
        let good = choice(&before, 1, 0x100);
        let mut bad = good.clone();
        bad.claim.expected_snapshot_sha256 = "00".repeat(32);
        requests.push(bad);
        let mut bad = good.clone();
        bad.claim.actor = form(0x200);
        requests.push(bad);
        for id in [4, 5, 6, 7, 8, 999] {
            let mut bad = good.clone();
            bad.claim.reference = reference(id);
            requests.push(bad);
        }
        for cell in [0x100, 0x402, 0x999] {
            let mut bad = good.clone();
            bad.cell = form(cell);
            requests.push(bad);
        }
        for bits in [0x7fc00031, 0x7f800000, 0xff800000] {
            let mut bad = good.clone();
            bad.position_bits[1] = bits;
            requests.push(bad);
            let mut bad = good.clone();
            bad.rotation_bits[2] = bits;
            requests.push(bad);
        }
        for bits in [0, 0x80000000, (-1f32).to_bits(), 0x7fc00031, 0x7f800000] {
            let mut bad = good.clone();
            bad.scale_bits = Some(bits);
            requests.push(bad);
        }
        let mut bad = good.clone();
        bad.claim.intent = Intent::Faithful {};
        requests.push(bad);
        for bad in requests {
            assert!(
                reference_intent::apply_private(
                    &before,
                    scripts,
                    content,
                    sources,
                    &bad,
                    WorldLimits::default(),
                    Limits::default()
                )
                .is_err()
            );
            assert_eq!(live.snapshot(), before);
        }
        let mut changed = before.clone();
        changed.state_revision += 1;
        assert!(matches!(
            reference_intent::apply_private(
                &changed,
                scripts,
                content,
                sources,
                &good,
                WorldLimits::default(),
                Limits::default()
            ),
            Err(Error::ContextChanged)
        ));
        let mut changed = before.clone();
        changed.campaign = fallout_runtime::identity::CampaignId::from_bytes([0x31; 16]).unwrap();
        assert!(matches!(
            reference_intent::apply_private(
                &changed,
                scripts,
                content,
                sources,
                &good,
                WorldLimits::default(),
                Limits::default()
            ),
            Err(Error::ContextChanged)
        ));
        let mut malformed = before.clone();
        malformed.references.push(malformed.references[0].clone());
        assert!(
            reference_intent::apply_private(
                &malformed,
                scripts,
                content,
                sources,
                &good,
                WorldLimits::default(),
                Limits::default()
            )
            .is_err()
        );
    });
    let changed = tempfile::tempdir().unwrap();
    fixture(changed.path(), 4);
    with_sources(dir.path(), |scripts, _, sources| {
        let before = world(scripts).snapshot();
        let request = choice(&before, 1, 0x100);
        with_sources(changed.path(), |other, other_content, other_sources| {
            for (catalogue, content, sources) in [
                (other, other_content, sources),
                (scripts, other_content, sources),
                (scripts, other_content, other_sources),
            ] {
                assert!(
                    reference_intent::apply_private(
                        &before,
                        catalogue,
                        content,
                        sources,
                        &request,
                        WorldLimits::default(),
                        Limits::default()
                    )
                    .is_err()
                );
            }
        });
    });
}
#[test]
fn exact_input_request_work_output_and_late_candidate_budgets() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, content, sources| {
        let before = world(scripts).snapshot();
        let request = choice(&before, 1, 0x100);
        let candidate = reference_intent::apply_private(
            &before,
            scripts,
            content,
            sources,
            &request,
            WorldLimits::default(),
            Limits::default(),
        )
        .unwrap();
        let exact = Limits {
            max_request_bytes: serde_json::to_vec(&request).unwrap().len(),
            max_snapshot_bytes: serde_json::to_vec(candidate.snapshot()).unwrap().len(),
            max_projection_bytes: serde_json::to_vec(&candidate).unwrap().len(),
            max_visits: candidate.visits(),
            ..Limits::default()
        };
        assert!(
            reference_intent::apply_private(
                &before,
                scripts,
                content,
                sources,
                &request,
                WorldLimits::default(),
                exact
            )
            .is_ok()
        );
        for label in [
            "request byte",
            "input snapshot byte",
            "candidate snapshot byte",
            "projection byte",
            "visit",
            "source",
        ] {
            let mut under = exact;
            match label {
                "request byte" => under.max_request_bytes -= 1,
                "input snapshot byte" => {
                    under.max_snapshot_bytes = serde_json::to_vec(&before).unwrap().len() - 1
                }
                "candidate snapshot byte" => under.max_snapshot_bytes -= 1,
                "projection byte" => under.max_projection_bytes -= 1,
                "visit" => under.max_visits -= 1,
                "source" => under.context.max_sources = 0,
                _ => unreachable!(),
            }
            assert!(
                matches!(reference_intent::apply_private(&before,scripts,content,sources,&request,WorldLimits::default(),under),Err(Error::Capacity(found)) if found==label)
            );
        }
        let under = WorldLimits {
            max_pending_events: 0,
            ..WorldLimits::default()
        };
        assert!(
            reference_intent::apply_private(
                &before,
                scripts,
                content,
                sources,
                &request,
                under,
                Limits::default()
            )
            .is_err()
        );
    });
}
#[test]
fn strict_wire_requires_every_choice_and_refuses_duplicate_and_unknown_intent_fields() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, _, _| {
        let before = world(scripts).snapshot();
        let request = choice(&before, 1, 0x100);
        let original = serde_json::to_value(request).unwrap();
        for field in [
            "scale_bits",
            "enabled",
            "position_bits",
            "rotation_bits",
            "cell",
            "claim",
        ] {
            let mut v = original.clone();
            v.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<reference_intent::Choice>(v).is_err(),
                "{field}"
            );
        }
        let mut v = original.clone();
        v["claim"]["intent"]["extra"] = serde_json::json!(1);
        assert!(serde_json::from_value::<reference_intent::Choice>(v).is_err());
        let duplicate = serde_json::to_string(&original).unwrap().replacen(
            "\"enabled\":false",
            "\"enabled\":false,\"enabled\":true",
            1,
        );
        assert!(serde_json::from_str::<reference_intent::Choice>(&duplicate).is_err());
    });
}
#[test]
fn export_complete_package_input_when_explicitly_requested() {
    let Some(path) = std::env::var_os("FALLOUT_ACTOR_INTERACTIONS_EVIDENCE_DIR") else {
        return;
    };
    let path = std::path::PathBuf::from(path);
    assert!(!path.join("Data").exists());
    fixture(&path, 3);
    with_sources(&path, |scripts, content, sources| {
        let before = world(scripts).snapshot();
        std::fs::write(
            path.join("snapshot.json"),
            before
                .encode(WorldLimits::default().max_snapshot_bytes)
                .unwrap(),
        )
        .unwrap();
        std::fs::write(
            path.join("reference-request.json"),
            serde_json::to_vec(&choice(&before, 1, 0x100)).unwrap(),
        )
        .unwrap();
        let candidate = reference_intent::apply_private(
            &before,
            scripts,
            content,
            sources,
            &choice(&before, 1, 0x100),
            WorldLimits::default(),
            Limits::default(),
        )
        .unwrap();
        std::fs::write(
            path.join("host-reference.json"),
            serde_json::to_vec(&candidate).unwrap(),
        )
        .unwrap();
        let bytes = serde_json::to_vec(&before).unwrap();
        assert_eq!(
            Snapshot::decode(&bytes, WorldLimits::default()).unwrap(),
            before
        );
    });
}

#[test]
fn borrowed_snapshot_bytes_refuse_before_malformed_source_name_normalization() {
    let dir = tempfile::tempdir().unwrap();
    fixture(dir.path(), 3);
    with_sources(dir.path(), |scripts, content, sources| {
        let before = world(scripts).snapshot();
        let request = choice(&before, 1, 0x100);
        let limits = Limits {
            max_snapshot_bytes: serde_json::to_vec(&before).unwrap().len() + 1,
            ..Limits::default()
        };
        let mut oversized = before.clone();
        oversized.references[7]
            .authored
            .as_mut()
            .unwrap()
            .origin_plugin = "malformed/source/".repeat(1024);
        // Earlier rows are valid. The final malformed origin must be refused
        // by borrowed byte admission, before canonical validation or restore.
        assert!(matches!(
            reference_intent::snapshot_sha256(&oversized, WorldLimits::default(), limits),
            Err(Error::Capacity("input snapshot byte"))
        ));
        assert!(matches!(
            reference_intent::apply_private(
                &oversized,
                scripts,
                content,
                sources,
                &request,
                WorldLimits::default(),
                limits
            ),
            Err(Error::Capacity("input snapshot byte"))
        ));
        assert_eq!(
            before.references[7]
                .authored
                .as_ref()
                .unwrap()
                .origin_plugin,
            "falloutnv.esm"
        );
    });
}
