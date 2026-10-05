//! Static binding of a frozen VMAD corpus to physical PEX definitions.
use fallout4_prep::{
    Error, Result, census,
    link::{Binding, Catalog, Lookup},
    pex, vmad,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Write},
    path::Path,
};

fn error(reason: &str) -> Error {
    Error::Unsupported(reason.into())
}
fn row(out: &mut impl Write, value: &Value) -> Result<()> {
    serde_json::to_writer(&mut *out, value)?;
    out.write_all(b"\n")?;
    Ok(())
}
fn json_file(path: &Path, value: &Value) -> Result<()> {
    serde_json::to_writer_pretty(
        OpenOptions::new().write(true).create_new(true).open(path)?,
        value,
    )?;
    Ok(())
}
fn local_file<'a>(row: &'a Value, key: &str) -> Result<&'a str> {
    let name = row[key]
        .as_str()
        .ok_or_else(|| error("missing local evidence filename"))?;
    if Path::new(name).components().count() != 1
        || Path::new(name).file_name().and_then(|p| p.to_str()) != Some(name)
    {
        return Err(error("invalid local evidence filename"));
    }
    Ok(name)
}
fn load_catalog(proof: &Path, corpus: &Value) -> Result<Catalog> {
    let mut catalog = Catalog::default();
    for (i, archive) in corpus["archives"]
        .as_array()
        .ok_or_else(|| error("missing archive census"))?
        .iter()
        .enumerate()
    {
        let pex_rows = archive["pex"]
            .as_array()
            .ok_or_else(|| error("missing PEX list"))?;
        if pex_rows.is_empty() {
            continue;
        }
        let name = archive["name"]
            .as_str()
            .ok_or_else(|| error("missing archive name"))?;
        eprintln!("Indexing Papyrus {name}");
        let mut expected = BTreeMap::new();
        for r in pex_rows {
            if expected
                .insert(
                    r["path"]
                        .as_str()
                        .ok_or_else(|| error("missing PEX path"))?,
                    &r["sha256"],
                )
                .is_some()
            {
                return Err(error("ambiguous saved PEX path"));
            }
        }
        let dir = proof.join(format!("archive-{i:03}")).join("pex");
        let manifest: Vec<(String, String, String)> =
            serde_json::from_reader(File::open(dir.join("manifest.json"))?)?;
        if manifest.len() != pex_rows.len() {
            return Err(error("PEX manifest cardinality changed"));
        }
        let mut seen = BTreeSet::new();
        for (file, path, hash) in manifest {
            if Path::new(&file).components().count() != 1 || !seen.insert(file.clone()) {
                return Err(error("unsafe or duplicate extracted PEX filename"));
            }
            if expected.remove(path.as_str()) != Some(&json!(hash)) {
                return Err(error("PEX manifest differs from frozen census"));
            }
            let file = dir.join(file);
            if fs::metadata(&file)?.len() > pex::Limits::default().file_bytes as u64 {
                return Err(error("PEX byte budget"));
            }
            let bytes = fs::read(file)?;
            if census::sha256(&bytes) != hash {
                return Err(error("extracted PEX fingerprint changed"));
            }
            let parsed = pex::parse(&bytes, &path, Default::default())?;
            catalog.add(&parsed, name, &path, &hash);
        }
    }
    Ok(catalog)
}
#[derive(Default)]
struct Counts {
    scripts: BTreeMap<String, u64>,
    properties: BTreeMap<String, u64>,
    fragments: BTreeMap<String, u64>,
    unresolved_scripts: BTreeMap<String, u64>,
}
fn count(map: &mut BTreeMap<String, u64>, key: &str) {
    *map.entry(key.into()).or_default() += 1;
}
fn script(
    catalog: &Catalog,
    s: &vmad::Script<'_>,
    origin: &Value,
    role: &str,
    counts: &mut Counts,
    out: &mut impl Write,
) -> Result<()> {
    let binding = catalog.bind(s.name);
    count(&mut counts.scripts, binding.status());
    if matches!(
        binding,
        Binding::Missing { .. } | Binding::Ambiguous { .. } | Binding::UnsupportedIdentifier { .. }
    ) {
        count(&mut counts.unresolved_scripts, &census::text(s.name));
    }
    row(
        out,
        &json!({"kind":"script","origin":origin,"role":role,"range":s.range,"name":census::text(s.name),"flags":s.flags,"binding":binding}),
    )?;
    for p in &s.properties {
        if let Binding::Unique { class } = binding {
            let resolved = catalog.lookup(class, p.name, false);
            count(&mut counts.properties, resolved.status());
            let target = if let Lookup::Found { class, member, .. } = resolved {
                let p = &catalog.classes()[class].properties[member];
                json!({"class":class,"member":member,"pex_range":p.range,"type":census::text(&p.type_name),"flags":p.flags,"auto_variable":p.auto_variable.as_deref().map(census::text)})
            } else {
                Value::Null
            };
            row(
                out,
                &json!({"kind":"property","origin":origin,"role":role,"script_range":s.range,"range":p.range,"name":census::text(p.name),"flags":p.flags,"vmad_type":p.value.type_code(),"binding":resolved,"target":target}),
            )?;
        } else {
            count(&mut counts.properties, "unbound_script");
            row(
                out,
                &json!({"kind":"property","origin":origin,"role":role,"script_range":s.range,"range":p.range,"name":census::text(p.name),"flags":p.flags,"vmad_type":p.value.type_code(),"binding":{"status":"unbound_script"}}),
            )?;
        }
    }
    Ok(())
}
fn fragment(
    catalog: &Catalog,
    class: &[u8],
    function: &[u8],
    origin: &Value,
    range: &std::ops::Range<usize>,
    counts: &mut Counts,
    out: &mut impl Write,
) -> Result<()> {
    let binding = catalog.bind(class);
    let (resolution, target) = if let Binding::Unique { class } = binding {
        let resolved = catalog.lookup(class, function, true);
        count(&mut counts.fragments, resolved.status());
        let target = if let Lookup::Found { class, member, .. } = resolved {
            let f = &catalog.classes()[class].methods[member];
            json!({"class":class,"method":member,"pex_range":f.range,"flags":f.flags})
        } else {
            Value::Null
        };
        (json!(resolved), target)
    } else {
        count(&mut counts.fragments, "unbound_script");
        (json!({"status":"unbound_script"}), Value::Null)
    };
    row(
        out,
        &json!({"kind":"fragment","origin":origin,"range":range,"script":census::text(class),"function":census::text(function),"class_binding":binding,"binding":resolution,"target":target}),
    )
}
fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err(error(
            "link-vmad <completed-content-proof> <vmad-scan-directory> <new-local-output>",
        ));
    }
    let proof = fs::canonicalize(&args[0])?;
    let scan_dir = fs::canonicalize(&args[1])?;
    let complete: Value = serde_json::from_reader(File::open(proof.join("complete.json"))?)?;
    let corpus_path = proof.join("census.json");
    if complete["status"] != "matched"
        || complete["census_sha256"] != census::hash_file(&corpus_path)?
    {
        return Err(error("completed content proof required"));
    }
    let corpus: Value = serde_json::from_reader(File::open(corpus_path)?)?;
    let scan: Value = serde_json::from_reader(File::open(scan_dir.join("scan.json"))?)?;
    if scan["decoded_all"] != true {
        return Err(error("complete VMAD scan required"));
    }
    for input in scan["inputs"]
        .as_array()
        .ok_or_else(|| error("scan inputs missing"))?
    {
        let input_path = Path::new(
            input["path"]
                .as_str()
                .ok_or_else(|| error("input path missing"))?,
        );
        let name = input_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| error("invalid input filename"))?;
        let expected = format!("Data/{name}");
        let files = corpus["files"]
            .as_array()
            .ok_or_else(|| error("missing frozen file list"))?;
        if files
            .iter()
            .find(|f| f["path"] == expected)
            .is_none_or(|f| f["sha256"] != input["sha256"])
        {
            return Err(error("VMAD and PEX proof snapshots disagree"));
        }
    }
    let requested = Path::new(&args[2]);
    let parent = fs::canonicalize(requested.parent().unwrap_or(Path::new(".")))?;
    if !parent.starts_with(fs::canonicalize("local")?)
        || parent.starts_with(&proof)
        || parent.starts_with(&scan_dir)
    {
        return Err(error(
            "new output must be private local/, outside input evidence",
        ));
    }
    let output = parent.join(
        requested
            .file_name()
            .ok_or_else(|| error("output name missing"))?,
    );
    fs::create_dir(&output)?;
    let catalog = load_catalog(&proof, &corpus)?;
    let definitions = catalog.definition_report();
    json_file(&output.join("classes.json"), &json!(definitions))?;
    let mut out = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join("bindings.jsonl"))?,
    );
    let mut counts = Counts::default();
    let mut fields = 0u64;
    let mut seen = BTreeSet::new();
    for line in BufReader::new(File::open(scan_dir.join("attachments.jsonl"))?).lines() {
        let saved: Value = serde_json::from_str(&line?)?;
        let filename = local_file(&saved, "file")?;
        if !seen.insert(filename.to_owned()) {
            return Err(error("duplicate VMAD filename"));
        }
        let file = scan_dir.join(filename);
        if fs::metadata(&file)?.len() > vmad::Limits::default().bytes as u64 {
            return Err(error("VMAD byte budget"));
        }
        let raw = fs::read(file)?;
        if saved["sha256"] != census::sha256(&raw) {
            return Err(error("VMAD payload fingerprint changed"));
        }
        let origin = &saved["origin"];
        let kind: [u8; 4] = origin["record_kind"]
            .as_str()
            .unwrap_or("")
            .as_bytes()
            .try_into()
            .map_err(|_| error("invalid record kind"))?;
        let a = vmad::parse(&raw, kind, filename, Default::default())?;
        for s in &a.scripts {
            script(&catalog, s, origin, "attached", &mut counts, &mut out)?;
        }
        if let Some(f) = &a.fragments {
            script(
                &catalog,
                &f.script,
                origin,
                "fragment_owner",
                &mut counts,
                &mut out,
            )?;
            for f in &f.fragments {
                fragment(
                    &catalog,
                    f.script,
                    f.function,
                    origin,
                    &f.range,
                    &mut counts,
                    &mut out,
                )?;
            }
            for p in &f.phases {
                fragment(
                    &catalog,
                    p.script,
                    p.function,
                    origin,
                    &p.range,
                    &mut counts,
                    &mut out,
                )?;
            }
            for a in &f.aliases {
                for s in &a.scripts {
                    script(
                        &catalog,
                        s,
                        origin,
                        &format!("alias@{}", a.range.start),
                        &mut counts,
                        &mut out,
                    )?;
                }
            }
        }
        fields += 1;
    }
    out.flush()?;
    if scan["vmad_fields"] != fields {
        return Err(error("VMAD cardinality mismatch"));
    }
    let result = json!({"schema":1,"status":"physical-binding-audit","vmad_fields":fields,"class_definitions":catalog.classes().len(),"duplicate_class_names":catalog.duplicate_names(),"script_bindings":counts.scripts,"property_bindings":counts.properties,"fragment_bindings":counts.fragments,"unresolved_script_names":counts.unresolved_scripts,"auto_property_findings":definitions.iter().filter(|d|d["auto_property_findings"].as_array().is_some_and(|f|!f.is_empty())).count(),"source_census_sha256":complete["census_sha256"],"source_vmad_manifest_sha256":census::hash_file(&scan_dir.join("attachments.jsonl"))?,"classes_sha256":census::hash_file(&output.join("classes.json"))?,"bindings_sha256":census::hash_file(&output.join("bindings.jsonl"))?,"limits":["Physical candidates only; no active archive/plugin winner order", "Default-state declarations only; no VM state dispatch", "Property declaration lookup only; no assignment/coercion/removal behavior", "Raw record identities; no ESL or master-slot resolution"]});
    json_file(&output.join("complete.json"), &result)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
