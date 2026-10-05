//! Emits physical PEX call and branch evidence without resolving runtime behavior.
use fallout4_prep::{
    Error, Result, census, executable,
    link::{Binding, Catalog, Class},
    pex,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::Path,
};

fn error(reason: &str) -> Error {
    Error::Unsupported(reason.into())
}

fn visit_pex(
    proof: &Path,
    corpus: &Value,
    mut visit: impl FnMut(&str, &str, &str, &pex::File<'_>) -> Result<()>,
) -> Result<()> {
    let archives = corpus["archives"]
        .as_array()
        .ok_or_else(|| error("missing archive census"))?;
    for (archive_index, archive) in archives.iter().enumerate() {
        let rows = archive["pex"]
            .as_array()
            .ok_or_else(|| error("missing PEX list"))?;
        if rows.is_empty() {
            continue;
        }
        let archive_name = archive["name"]
            .as_str()
            .ok_or_else(|| error("missing archive name"))?;
        let mut expected = BTreeMap::new();
        for row in rows {
            if expected
                .insert(
                    row["path"]
                        .as_str()
                        .ok_or_else(|| error("missing PEX path"))?,
                    row["sha256"]
                        .as_str()
                        .ok_or_else(|| error("missing PEX hash"))?,
                )
                .is_some()
            {
                return Err(error("duplicate PEX path in source census"));
            }
        }
        let directory = proof
            .join(format!("archive-{archive_index:03}"))
            .join("pex");
        let manifest: Vec<(String, String, String)> =
            serde_json::from_reader(File::open(directory.join("manifest.json"))?)?;
        if manifest.len() != rows.len() {
            return Err(error("PEX manifest cardinality changed"));
        }
        let mut seen_files = BTreeSet::new();
        for (filename, path, hash) in manifest {
            if Path::new(&filename).components().count() != 1
                || !seen_files.insert(filename.clone())
            {
                return Err(error("unsafe or duplicate extracted PEX filename"));
            }
            if expected.remove(path.as_str()) != Some(hash.as_str()) {
                return Err(error("PEX manifest differs from frozen census"));
            }
            let file_path = directory.join(filename);
            if fs::metadata(&file_path)?.len() > pex::Limits::default().file_bytes as u64 {
                return Err(error("PEX byte budget"));
            }
            let bytes = fs::read(file_path)?;
            if census::sha256(&bytes) != hash {
                return Err(error("extracted PEX fingerprint changed"));
            }
            let parsed = pex::parse(&bytes, &path, Default::default())?;
            visit(archive_name, &path, &hash, &parsed)?;
        }
        if !expected.is_empty() {
            return Err(error("PEX manifest omits frozen census entries"));
        }
    }
    Ok(())
}

fn identifier(value: Option<&pex::Value>, strings: &[&[u8]]) -> Option<Vec<u8>> {
    let pex::Value::Identifier(index) = value? else {
        return None;
    };
    strings.get(*index as usize).map(|bytes| bytes.to_vec())
}

fn value_json(value: Option<&pex::Value>, strings: &[&[u8]]) -> Value {
    let Some(value) = value else {
        return Value::Null;
    };
    match value {
        pex::Value::None => json!({"tag":"none"}),
        pex::Value::Identifier(index) => {
            json!({"tag":"identifier","index":index,"text":strings.get(*index as usize).map(|s|census::text(s))})
        }
        pex::Value::String(index) => {
            json!({"tag":"string","index":index,"text":strings.get(*index as usize).map(|s|census::text(s))})
        }
        pex::Value::Integer(value) => json!({"tag":"integer","value":value}),
        pex::Value::FloatBits(bits) => json!({"tag":"float_bits","bits":bits}),
        pex::Value::BoolByte(value) => json!({"tag":"bool_byte","value":value}),
    }
}

#[derive(Clone, Copy)]
struct CallShape {
    class_index: Option<usize>,
    method_index: usize,
    receiver_index: Option<usize>,
    destination_index: usize,
}
fn call_shape(opcode: u8) -> Option<CallShape> {
    match opcode {
        23 => Some(CallShape {
            class_index: None,
            method_index: 0,
            receiver_index: Some(1),
            destination_index: 2,
        }),
        24 => Some(CallShape {
            class_index: None,
            method_index: 0,
            receiver_index: None,
            destination_index: 1,
        }),
        25 => Some(CallShape {
            class_index: Some(0),
            method_index: 1,
            receiver_index: None,
            destination_index: 2,
        }),
        _ => None,
    }
}

fn class_ids(binding: &Binding) -> Vec<usize> {
    match binding {
        Binding::Unique { class } => vec![*class],
        Binding::Ambiguous { classes, .. } => classes.clone(),
        _ => Vec::new(),
    }
}

fn declaration_candidates(
    classes: &[Class],
    binding: &Binding,
    class_name: Option<&[u8]>,
    method_name: Option<&[u8]>,
) -> (String, Vec<Value>) {
    let (Some(class_name), Some(method_name)) = (class_name, method_name) else {
        return ("call_operands_not_identifiers".into(), Vec::new());
    };
    if !class_name.is_ascii()
        || class_name.contains(&0)
        || !method_name.is_ascii()
        || method_name.contains(&0)
    {
        return ("unsupported_identifier".into(), Vec::new());
    }
    match binding {
        Binding::Missing { .. } => {
            return ("class_missing_from_physical_catalog".into(), Vec::new());
        }
        Binding::UnsupportedIdentifier { .. } => {
            return ("unsupported_identifier".into(), Vec::new());
        }
        Binding::Empty => return ("empty_class_identifier".into(), Vec::new()),
        Binding::Unique { .. } | Binding::Ambiguous { .. } => {}
    }
    let mut matches = Vec::new();
    for class_id in class_ids(binding) {
        let Some(class) = classes.get(class_id) else {
            continue;
        };
        for (declaration_id, declaration) in class.declarations.iter().enumerate() {
            if declaration.name.eq_ignore_ascii_case(method_name) {
                matches.push(json!({"class":class_id,"declaration":declaration_id}));
            }
        }
    }
    let status = if matches.is_empty() {
        "no_same_name_direct_declaration"
    } else if matches.len() == 1 && matches!(binding, Binding::Unique { .. }) {
        "one_same_name_direct_declaration"
    } else {
        "unselected_class_or_declaration_binding"
    };
    (status.into(), matches)
}

fn declaration_index(catalog: &Catalog) -> Vec<Value> {
    catalog
        .classes()
        .iter()
        .enumerate()
        .map(|(class_id, class)| {
            json!({
                "class":class_id,
                "name":census::text(&class.name),
                "source":class.source,
                "declarations":class.declarations.iter().enumerate().map(|(declaration_id,d)| json!({
                    "declaration":declaration_id,
                    "name":census::text(&d.name),
                    "state":d.state.as_deref().map(census::text),
                    "kind":d.kind,
                    "range":d.range,
                    "flags":d.flags,
                    "native_declaration":d.native(),
                    "return_type":census::text(&d.return_type),
                    "parameter_count":d.parameter_count
                })).collect::<Vec<_>>()
            })
        })
        .collect()
}

#[derive(Default)]
struct Counts {
    files: u64,
    objects: u64,
    functions: u64,
    instructions: u64,
    opcode_histogram: BTreeMap<u8, u64>,
    branches: u64,
    branches_to_instruction: u64,
    branches_to_function_end: u64,
    calls: u64,
    call_method: u64,
    call_parent: u64,
    call_static: u64,
    call_static_native_candidates: u64,
    statuses: BTreeMap<String, u64>,
}
fn inc(map: &mut BTreeMap<String, u64>, name: &str) {
    *map.entry(name.into()).or_default() += 1;
}

fn fo4_struct_array_opcode_counts(histogram: &BTreeMap<u8, u64>) -> BTreeMap<u8, u64> {
    [31, 32, 33, 38, 39]
        .into_iter()
        .map(|opcode| (opcode, histogram.get(&opcode).copied().unwrap_or(0)))
        .collect()
}

fn emit_file(
    archive: &str,
    path: &str,
    hash: &str,
    file: &pex::File<'_>,
    catalog: &Catalog,
    counts: &mut Counts,
    out: &mut impl Write,
) -> Result<()> {
    counts.files += 1;
    counts.objects += file.objects.len() as u64;
    for (object_id, object) in file.objects.iter().enumerate() {
        let class = &file.strings[object.name as usize];
        for (function_id, function) in object.functions.iter().enumerate() {
            counts.functions += 1;
            for (pc, instruction) in function.instructions.iter().enumerate() {
                counts.instructions += 1;
                *counts
                    .opcode_histogram
                    .entry(instruction.opcode)
                    .or_default() += 1;
                if let Some(destination) = executable::branch_destination(
                    instruction,
                    pc,
                    function.instructions.len(),
                    path,
                )? {
                    counts.branches += 1;
                    match destination {
                        executable::BranchDestination::Instruction { .. } => {
                            counts.branches_to_instruction += 1;
                        }
                        executable::BranchDestination::FunctionEnd => {
                            counts.branches_to_function_end += 1;
                        }
                    }
                    let displacement = match instruction
                        .arguments
                        .get(if instruction.opcode == 20 { 0 } else { 1 })
                    {
                        Some(pex::Value::Integer(value)) => *value,
                        _ => return Err(error("validated branch displacement changed")),
                    };
                    serde_json::to_writer(
                        &mut *out,
                        &json!({
                            "kind":"branch",
                            "source":{"archive":archive,"path":path,"sha256":hash,"object":object_id},
                            "class":census::text(class),
                        "function":{"index":function_id,"name":census::text(file.strings[function.name as usize]),"state":function.state.map(|s|census::text(file.strings[s as usize])),"kind":function.kind,"range":function.range},
                            "instruction":{"index":pc,"offset":instruction.offset,"opcode":instruction.opcode},
                            "displacement_in_instructions":displacement,
                            "destination":destination
                        }),
                    )?;
                    out.write_all(b"\n")?;
                }
                if !matches!(instruction.opcode, 23..=25) {
                    continue;
                }
                counts.calls += 1;
                let shape = call_shape(instruction.opcode)
                    .ok_or_else(|| error("call opcode lacks its pinned PEX operand shape"))?;
                let call_kind = match instruction.opcode {
                    23 => {
                        counts.call_method += 1;
                        "call_method"
                    }
                    24 => {
                        counts.call_parent += 1;
                        "call_parent"
                    }
                    25 => {
                        counts.call_static += 1;
                        "call_static"
                    }
                    _ => unreachable!(),
                };
                let class_name = shape
                    .class_index
                    .and_then(|index| identifier(instruction.arguments.get(index), &file.strings));
                let method_name =
                    identifier(instruction.arguments.get(shape.method_index), &file.strings);
                let receiver = shape
                    .receiver_index
                    .and_then(|index| instruction.arguments.get(index));
                let binding = if instruction.opcode == 25 {
                    class_name.as_deref().map(|name| catalog.bind(name))
                } else {
                    None
                };
                let (status, candidates) = if let Some(binding) = &binding {
                    declaration_candidates(
                        catalog.classes(),
                        binding,
                        class_name.as_deref(),
                        method_name.as_deref(),
                    )
                } else {
                    (
                        match instruction.opcode {
                            23 => "receiver_runtime_dispatch_unresolved",
                            24 => "parent_relative_dispatch_unresolved",
                            _ => "call_operands_not_identifiers",
                        }
                        .into(),
                        Vec::new(),
                    )
                };
                if instruction.opcode == 25
                    && candidates.iter().any(|candidate| {
                        candidate["class"]
                            .as_u64()
                            .zip(candidate["declaration"].as_u64())
                            .is_some_and(|(c, d)| {
                                catalog
                                    .classes()
                                    .get(c as usize)
                                    .and_then(|cl| cl.declarations.get(d as usize))
                                    .is_some_and(|decl| decl.native())
                            })
                    })
                {
                    counts.call_static_native_candidates += 1;
                }
                inc(&mut counts.statuses, &status);
                let row = json!({
                    "kind":"call",
                    "call_kind":call_kind,
                    "source":{"archive":archive,"path":path,"sha256":hash,"object":object_id},
                    "class":census::text(class),
                    "function":{"index":function_id,"name":census::text(file.strings[function.name as usize]),"state":function.state.map(|s|census::text(file.strings[s as usize])),"kind":function.kind,"range":function.range},
                    "instruction":{"index":pc,"offset":instruction.offset,"opcode":instruction.opcode},
                    "operands":instruction.arguments.iter().map(|value|value_json(Some(value), &file.strings)).collect::<Vec<_>>(),
                    "destination_operand":value_json(instruction.arguments.get(shape.destination_index), &file.strings),
                    "class_operand":class_name.as_deref().map(census::text),
                    "method_operand":method_name.as_deref().map(census::text),
                    "receiver_operand":receiver.map(|value|value_json(Some(value), &file.strings)),
                    "class_binding":binding,
                    "physical_declaration_candidate_status":status,
                    "physical_declaration_candidates":candidates,
                    "vararg_count":instruction.varargs.len(),
                    "limits":["Static calls list exact-name declarations on physically named classes; no inheritance, state dispatch, overload, arity or return-type resolution","Method/parent calls remain unresolved","No native host function is implemented"]
                });
                serde_json::to_writer(&mut *out, &row)?;
                out.write_all(b"\n")?;
            }
        }
    }
    Ok(())
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(error(
            "link-pex <completed-content-proof> <new-local-output>",
        ));
    }
    let proof = fs::canonicalize(&args[0])?;
    let complete_path = proof.join("complete.json");
    let proof_complete: Value = serde_json::from_reader(File::open(&complete_path)?)?;
    let corpus_path = proof.join("census.json");
    let census_hash = census::hash_file(&corpus_path)?;
    if proof_complete["status"] != "matched" || proof_complete["census_sha256"] != census_hash {
        return Err(error("completed content proof required"));
    }
    let corpus: Value = serde_json::from_reader(File::open(&corpus_path)?)?;
    let requested = Path::new(&args[1]);
    let parent = fs::canonicalize(requested.parent().unwrap_or(Path::new(".")))?;
    let local = fs::canonicalize("local")?;
    if !parent.starts_with(&local) || parent.starts_with(&proof) {
        return Err(error(
            "new output must be private local/, outside input proof",
        ));
    }
    let output = parent.join(
        requested
            .file_name()
            .ok_or_else(|| error("output name missing"))?,
    );
    fs::create_dir(&output)?;

    let mut catalog = Catalog::default();
    visit_pex(&proof, &corpus, |archive, path, hash, file| {
        catalog.add(file, archive, path, hash);
        Ok(())
    })?;
    let declarations = declaration_index(&catalog);
    let declarations_path = output.join("declarations.json");
    serde_json::to_writer_pretty(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&declarations_path)?,
        &declarations,
    )?;

    let output_path = output.join("executable.jsonl");
    let mut writer = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output_path)?,
    );
    let mut counts = Counts::default();
    visit_pex(&proof, &corpus, |archive, path, hash, file| {
        emit_file(
            archive,
            path,
            hash,
            file,
            &catalog,
            &mut counts,
            &mut writer,
        )
    })?;
    writer.flush()?;
    let result = json!({
        "schema":1,
        "status":"static_pex_call_and_branch_inventory",
        "source_census_sha256":census_hash,
        "source_proof_complete_sha256":census::hash_file(&complete_path)?,
        "class_definitions":catalog.classes().len(),
        "counts":{
            "files":counts.files,
            "objects":counts.objects,
            "functions":counts.functions,
            "instructions":counts.instructions,
            "opcode_histogram":&counts.opcode_histogram,
            "fo4_struct_array_opcode_counts_by_id":fo4_struct_array_opcode_counts(&counts.opcode_histogram),
            "branches":counts.branches,
            "branches_to_instruction":counts.branches_to_instruction,
            "branches_to_function_end":counts.branches_to_function_end,
            "calls":counts.calls,
            "call_method":counts.call_method,
            "call_parent":counts.call_parent,
            "call_static":counts.call_static,
            "call_static_with_native_declaration_candidate":counts.call_static_native_candidates
        },
        "call_candidate_statuses":counts.statuses,
        "declarations_sha256":census::hash_file(&declarations_path)?,
        "executable_sha256":census::hash_file(&output_path)?,
        "limits":[
            "Inventory covers only the frozen physical PEX corpus, not active overrides or runtime behavior",
            "CALLSTATIC candidates are exact-name declarations on named physical classes only; no inheritance, state dispatch, overload, arity or return-type rules are inferred",
            "CALLMETHOD receiver dispatch and CALLPARENT dispatch remain unresolved",
            "Branch targets are instruction indices; one-past-end is retained as a boundary without assigning continuation semantics",
            "Native declaration candidates are not implemented host APIs"
        ]
    });
    serde_json::to_writer_pretty(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join("complete.json"))?,
        &result,
    )?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallout4_prep::link::{Declaration, Source};

    fn class(name: &[u8], declarations: Vec<Declaration>) -> Class {
        Class {
            name: name.to_vec(),
            parent: Vec::new(),
            source: Source {
                archive: "fixture.ba2".into(),
                path: "Scripts/Fixture.pex".into(),
                sha256: "fixture-hash".into(),
                object: 0,
            },
            properties: Vec::new(),
            variables: Vec::new(),
            methods: Vec::new(),
            declarations,
        }
    }

    fn declaration(name: &[u8], state: Option<&[u8]>) -> Declaration {
        Declaration {
            name: name.to_vec(),
            state: state.map(ToOwned::to_owned),
            kind: "method",
            range: 10..20,
            flags: 0,
            return_type: b"None".to_vec(),
            parameter_count: 0,
        }
    }

    #[test]
    fn call_operand_shapes_follow_pinned_pex_builder_order() {
        let strings: [&[u8]; 3] = [b"Utility", b"Run", b"destination"];
        let static_call = call_shape(25).unwrap();
        let values = [
            pex::Value::Identifier(0),
            pex::Value::Identifier(1),
            pex::Value::Identifier(2),
        ];
        assert_eq!(
            identifier(Some(&values[static_call.class_index.unwrap()]), &strings).as_deref(),
            Some(b"Utility".as_slice())
        );
        assert_eq!(
            identifier(Some(&values[static_call.method_index]), &strings).as_deref(),
            Some(b"Run".as_slice())
        );
        assert_eq!(
            identifier(Some(&values[static_call.destination_index]), &strings).as_deref(),
            Some(b"destination".as_slice())
        );

        let method_call = call_shape(23).unwrap();
        assert_eq!(method_call.method_index, 0);
        assert_eq!(method_call.receiver_index, Some(1));
        assert_eq!(method_call.destination_index, 2);
        let parent_call = call_shape(24).unwrap();
        assert_eq!(parent_call.method_index, 0);
        assert_eq!(parent_call.destination_index, 1);
    }

    #[test]
    fn opcode_census_keeps_fo4_struct_and_array_ids_distinct() {
        let mut histogram = BTreeMap::new();
        for opcode in [31, 32, 33, 38, 39, 31] {
            *histogram.entry(opcode).or_insert(0u64) += 1;
        }
        assert_eq!(
            fo4_struct_array_opcode_counts(&histogram),
            BTreeMap::from([(31, 2), (32, 1), (33, 1), (38, 1), (39, 1)])
        );
    }

    #[test]
    fn static_call_inventory_keeps_state_and_class_collisions_unselected() {
        let unique = class(b"Utility", vec![declaration(b"Run", None)]);
        let status = declaration_candidates(
            std::slice::from_ref(&unique),
            &Binding::Unique { class: 0 },
            Some(b"utility"),
            Some(b"RUN"),
        );
        assert_eq!(status.0, "one_same_name_direct_declaration");
        assert_eq!(status.1, vec![json!({"class":0,"declaration":0})]);

        let state_collision = class(
            b"Utility",
            vec![
                declaration(b"Run", None),
                declaration(b"RUN", Some(b"Busy")),
            ],
        );
        let status = declaration_candidates(
            std::slice::from_ref(&state_collision),
            &Binding::Unique { class: 0 },
            Some(b"Utility"),
            Some(b"Run"),
        );
        assert_eq!(status.0, "unselected_class_or_declaration_binding");
        assert_eq!(status.1.len(), 2);
    }

    #[test]
    fn static_call_inventory_preserves_missing_ambiguous_and_unsupported_cases() {
        let classes = vec![
            class(b"Utility", vec![declaration(b"Run", None)]),
            class(b"UTILITY", vec![declaration(b"Run", None)]),
        ];
        let ambiguous = Binding::Ambiguous {
            name: "Utility".into(),
            classes: vec![0, 1],
        };
        let status = declaration_candidates(&classes, &ambiguous, Some(b"Utility"), Some(b"Run"));
        assert_eq!(status.0, "unselected_class_or_declaration_binding");
        assert_eq!(status.1.len(), 2);

        assert_eq!(
            declaration_candidates(
                &classes,
                &Binding::Missing {
                    name: "Absent".into(),
                },
                Some(b"Absent"),
                Some(b"Run"),
            )
            .0,
            "class_missing_from_physical_catalog"
        );
        assert_eq!(
            declaration_candidates(
                &classes,
                &Binding::Unique { class: 0 },
                Some(b"Utility"),
                Some(b"\xff"),
            )
            .0,
            "unsupported_identifier"
        );
        assert_eq!(
            declaration_candidates(&classes, &Binding::Unique { class: 0 }, None, None).0,
            "call_operands_not_identifiers"
        );
    }
}
