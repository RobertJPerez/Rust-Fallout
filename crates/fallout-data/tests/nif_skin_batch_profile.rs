//! Bounded source-backed comparison of individual and prepared multi-geometry skin evaluation.
//! This stays ignored unless explicit private local source/evidence paths are supplied.
use fallout_data::nif_skin::{self, pose};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs::OpenOptions, io::Write, path::PathBuf, time::Instant};

const VISIBLE_FIELDS: [&str; 9] = [
    "geometry",
    "instance",
    "skeleton_root",
    "weights",
    "skin_to_source_world",
    "palette",
    "positions",
    "normals",
    "weight_sums",
];

fn sha256_bytes(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

fn digest_from_hex(value: &str) -> [u8; 32] {
    assert_eq!(value.len(), 64);
    let mut digest = [0u8; 32];
    for (index, byte) in digest.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).unwrap();
    }
    digest
}

fn request(geometry: u32) -> pose::Request {
    pose::Request {
        geometry,
        weights: pose::WeightPolicy::PreserveRawNonnegative,
    }
}

#[test]
#[ignore = "Requires an explicit local source-manifest and a new private profile report path"]
fn prepared_multi_geometry_pose_matches_individual_output_and_refusals() {
    let manifest_path = PathBuf::from(
        std::env::var("ASSET_SKIN_SOURCE_MANIFEST").expect("explicit source manifest required"),
    )
    .canonicalize()
    .unwrap();
    let output_path = PathBuf::from(
        std::env::var("ASSET_SKIN_PROFILE_REPORT").expect("new private report path required"),
    );
    let local = std::env::current_dir()
        .unwrap()
        .join("../../local")
        .canonicalize()
        .unwrap();
    let manifest_parent = manifest_path.parent().unwrap().canonicalize().unwrap();
    let output_parent = output_path.parent().unwrap().canonicalize().unwrap();
    assert!(
        manifest_path.starts_with(&local),
        "manifest must stay local"
    );
    assert!(
        manifest_parent.starts_with(&local),
        "inputs must stay local"
    );
    assert!(output_parent.starts_with(&local), "report must stay local");
    assert!(!output_path.exists(), "profile report must be new");

    let manifest: Value = serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
    assert_eq!(manifest["schema_version"].as_u64(), Some(2));
    let samples = manifest["samples"].as_array().unwrap();
    let selected = samples
        .iter()
        .filter(|sample| {
            sample["skin_owner_count"].as_u64().unwrap_or(0) >= 2
                && sample["max_raw_influences_per_vertex"]
                    .as_u64()
                    .unwrap_or(0)
                    > 4
        })
        .max_by_key(|sample| sample["skin_owner_count"].as_u64().unwrap_or(0))
        .expect("source manifest needs a multi-owner rig with more than four raw influences");
    let expected_sha256 = selected["sha256"].as_str().unwrap();
    let expected_digest = digest_from_hex(expected_sha256);
    let filename = selected["file"].as_str().unwrap();
    assert_eq!(
        std::path::Path::new(filename)
            .file_name()
            .and_then(|name| name.to_str()),
        Some(filename)
    );
    let input_dir = manifest_parent.join("inputs").canonicalize().unwrap();
    assert!(input_dir.starts_with(&local));
    let input_path = input_dir.join(filename).canonicalize().unwrap();
    assert_eq!(input_path.parent(), Some(input_dir.as_path()));
    let bytes = std::fs::read(&input_path).unwrap();
    let digest = sha256_bytes(&bytes);
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), expected_sha256);
    assert_eq!(digest, expected_digest);
    assert_eq!(
        bytes.len() as u64,
        selected["decoded_bytes"].as_u64().unwrap()
    );

    let source = format!("skin source sha256 {expected_sha256}");
    let (_, decoded) = nif_skin::decode(&bytes, &source).unwrap();
    assert_eq!(
        decoded.owners.len() as u64,
        selected["skin_owner_count"].as_u64().unwrap()
    );
    let owner_count = decoded.owners.len();
    assert!(owner_count > 1 && owner_count <= 64);

    let mut baseline_attempts = 0usize;
    let mut baseline_refusals = Vec::new();
    let mut baseline = Vec::<(pose::Request, Value)>::new();
    let baseline_start = Instant::now();
    for owner in &decoded.owners {
        if baseline.len() == 16 {
            break;
        }
        let selected_request = request(owner.geometry);
        baseline_attempts += 1;
        match pose::evaluate(&bytes, &source, selected_request, Default::default()) {
            Ok(evaluation) => {
                baseline.push((selected_request, serde_json::to_value(evaluation).unwrap()))
            }
            Err(error) => baseline_refusals.push(json!({
                "geometry": owner.geometry,
                "reason": error.to_string()
            })),
        }
    }
    let baseline_nanos = baseline_start.elapsed().as_nanos();
    assert!(
        baseline.len() >= 2,
        "need multiple accepted source geometries"
    );
    let requests: Vec<_> = baseline.iter().map(|(request, _)| *request).collect();

    let preparation_limits = pose::PreparationLimits::default();
    let preparation_start = Instant::now();
    let prepared = pose::PreparedSkinSource::prepare(&bytes, &source, preparation_limits).unwrap();
    let preparation_nanos = preparation_start.elapsed().as_nanos();
    let preparation_usage = prepared.usage();
    assert_eq!(preparation_usage.binding_decodes, 1);
    assert_eq!(preparation_usage.scene_decodes, 1);
    assert!(preparation_usage.retained_bytes <= preparation_limits.array_bytes);
    assert!(preparation_usage.work_units <= preparation_limits.work_units);

    let batch_limits = pose::BatchEvaluationLimits::default();
    let batch_start = Instant::now();
    let batch = prepared
        .evaluate_many(&source, expected_digest, &requests, batch_limits)
        .unwrap();
    let batch_nanos = batch_start.elapsed().as_nanos();
    assert_eq!(batch.geometries.len(), baseline.len());
    assert!(batch.retained_bytes <= batch_limits.array_bytes);
    assert!(batch.work_units <= batch_limits.work_units);
    let mut distinct_palettes = BTreeSet::new();
    let mut geometry_rows = Vec::new();
    for (index, (selected_request, individual)) in baseline.iter().enumerate() {
        let cached = serde_json::to_value(&batch.geometries[index]).unwrap();
        for field in VISIBLE_FIELDS {
            assert_eq!(
                individual.get(field),
                cached.get(field),
                "prepared output differs for geometry {} field {field}",
                selected_request.geometry
            );
        }
        distinct_palettes.insert(serde_json::to_string(cached.get("palette").unwrap()).unwrap());
        geometry_rows.push(json!({
            "geometry": selected_request.geometry,
            "palette_bones": batch.geometries[index].palette.len(),
            "vertices": batch.geometries[index].positions.len(),
            "raw_weight_sums": batch.geometries[index].weight_sums.len()
        }));
    }

    let missing_geometry = request(u32::MAX);
    let individual_missing = pose::evaluate(&bytes, &source, missing_geometry, Default::default())
        .unwrap_err()
        .to_string();
    let mut invalid_batch_requests = requests.clone();
    invalid_batch_requests.push(missing_geometry);
    let prepared_missing = prepared
        .evaluate_many(
            &source,
            expected_digest,
            &invalid_batch_requests,
            batch_limits,
        )
        .unwrap_err()
        .to_string();
    assert!(individual_missing.contains("no decoded skin owner"));
    assert!(prepared_missing.contains("no decoded skin owner"));

    let mut wrong_digest = expected_digest;
    wrong_digest[0] ^= 1;
    let individual_stale = pose::evaluate_many(
        &bytes,
        &source,
        wrong_digest,
        &requests,
        pose::BatchLimits::default(),
    )
    .unwrap_err()
    .to_string();
    let prepared_stale = prepared
        .evaluate_many(&source, wrong_digest, &requests, batch_limits)
        .unwrap_err()
        .to_string();
    assert!(individual_stale.contains("SHA256 differs"));
    assert!(prepared_stale.contains("SHA256 differs"));

    let report = json!({
        "schema_version": 1,
        "scope": "bounded original-source CPU comparison; exact consumed skin positions/normals/palettes/weights; no GPU or gameplay proof",
        "source": {
            "category": selected["category"],
            "sha256": expected_sha256,
            "bytes": bytes.len(),
            "owner_geometries": owner_count,
            "manifest_max_raw_influences_per_vertex": selected["max_raw_influences_per_vertex"],
            "input_instance_types": selected["decoded_skin_structures"]
        },
        "comparison": {
            "successful_geometries": baseline.len(),
            "one_shot_attempts": baseline_attempts,
            "one_shot_binding_and_scene_decodes": baseline_attempts,
            "prepared_binding_decodes": preparation_usage.binding_decodes,
            "prepared_scene_decodes": preparation_usage.scene_decodes,
            "exact_visible_fields": VISIBLE_FIELDS,
            "distinct_palette_signatures": distinct_palettes.len(),
            "geometries": geometry_rows,
            "one_shot_elapsed_nanoseconds": baseline_nanos,
            "prepare_elapsed_nanoseconds": preparation_nanos,
            "prepared_batch_elapsed_nanoseconds": batch_nanos,
            "one_shot_refusals": baseline_refusals
        },
        "memory_and_work": {
            "preparation_retained_bytes": preparation_usage.retained_bytes,
            "preparation_retained_limit_bytes": preparation_limits.array_bytes,
            "preparation_work_units": preparation_usage.work_units,
            "preparation_work_limit_units": preparation_limits.work_units,
            "source_binding_retained_bytes": preparation_usage.source_binding_retained_bytes,
            "batch_retained_bytes": batch.retained_bytes,
            "batch_retained_limit_bytes": batch_limits.array_bytes,
            "batch_work_units": batch.work_units,
            "batch_work_limit_units": batch_limits.work_units
        },
        "rejections": {
            "missing_geometry_individual": individual_missing,
            "missing_geometry_prepared_batch": prepared_missing,
            "wrong_sha_individual_batch": individual_stale,
            "wrong_sha_prepared_batch": prepared_stale,
            "matching_refusal_classes": true
        },
        "retail_behavior_verified": false
    });
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
        .unwrap();
    serde_json::to_writer_pretty(&mut file, &report).unwrap();
    writeln!(file).unwrap();
    println!(
        "Compared {} geometries across {} one-shot decodes and one prepared decode; {} palette signatures; same positions/normals and refusal classes",
        baseline.len(),
        baseline_attempts,
        distinct_palettes.len()
    );
}
