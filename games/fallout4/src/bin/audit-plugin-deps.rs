//! Audits physical Fallout 4 plugin master declarations without choosing activation order.
use fallout4_prep::{Error, Result, census, formid};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fs::{self, File, OpenOptions},
    path::Path,
};

const MAX_CENSUS_BYTES: u64 = 256 * 1024 * 1024;
const MAX_PLUGINS: usize = 65_536;
const MAX_MASTERS_PER_PLUGIN: usize = 4096;

fn error(reason: &str) -> Error {
    Error::Unsupported(reason.into())
}

#[derive(Debug)]
struct Plugin {
    name: String,
    sha256: String,
    header_flags: u32,
    masters: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
enum MasterBinding {
    Unique { plugin: usize },
    Missing { name: String },
    Ambiguous { name: String, plugins: Vec<usize> },
    UnsupportedIdentifier { name: String },
}
impl MasterBinding {
    fn status(&self) -> &'static str {
        match self {
            Self::Unique { .. } => "unique",
            Self::Missing { .. } => "missing",
            Self::Ambiguous { .. } => "ambiguous",
            Self::UnsupportedIdentifier { .. } => "unsupported_identifier",
        }
    }
}

fn name_index(plugins: &[Plugin]) -> BTreeMap<String, Vec<usize>> {
    let mut index: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (id, plugin) in plugins.iter().enumerate() {
        index
            .entry(plugin.name.to_ascii_lowercase())
            .or_default()
            .push(id);
    }
    index
}

fn bind_master(name: &str, index: &BTreeMap<String, Vec<usize>>) -> MasterBinding {
    if name.is_empty() || !name.is_ascii() || name.contains('\0') || name.contains('\\') {
        return MasterBinding::UnsupportedIdentifier { name: name.into() };
    }
    match index
        .get(&name.to_ascii_lowercase())
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        [] => MasterBinding::Missing { name: name.into() },
        [plugin] => MasterBinding::Unique { plugin: *plugin },
        plugins => MasterBinding::Ambiguous {
            name: name.into(),
            plugins: plugins.to_vec(),
        },
    }
}

fn graph_cycles(plugins: &[Plugin], index: &BTreeMap<String, Vec<usize>>) -> Vec<Vec<usize>> {
    fn visit(
        id: usize,
        plugins: &[Plugin],
        index: &BTreeMap<String, Vec<usize>>,
        states: &mut [u8],
        stack: &mut Vec<usize>,
        found: &mut BTreeSet<Vec<usize>>,
    ) {
        if states[id] == 2 {
            return;
        }
        if states[id] == 1 {
            if let Some(start) = stack.iter().position(|candidate| *candidate == id) {
                let mut cycle = stack[start..].to_vec();
                if let Some((minimum, _)) = cycle.iter().enumerate().min_by_key(|(_, v)| *v) {
                    cycle.rotate_left(minimum);
                }
                found.insert(cycle);
            }
            return;
        }
        states[id] = 1;
        stack.push(id);
        for master in &plugins[id].masters {
            if let MasterBinding::Unique { plugin } = bind_master(master, index) {
                visit(plugin, plugins, index, states, stack, found);
            }
        }
        stack.pop();
        states[id] = 2;
    }

    let mut states = vec![0; plugins.len()];
    let mut found = BTreeSet::new();
    for id in 0..plugins.len() {
        visit(id, plugins, index, &mut states, &mut Vec::new(), &mut found);
    }
    found.into_iter().collect()
}

fn load_plugins(corpus: &Value) -> Result<Vec<Plugin>> {
    let rows = corpus["plugins"]
        .as_array()
        .ok_or_else(|| error("frozen plugin census is missing"))?;
    if rows.len() > MAX_PLUGINS {
        return Err(error("plugin census exceeds node budget"));
    }
    let files = corpus["files"]
        .as_array()
        .ok_or_else(|| error("frozen file fingerprint list is missing"))?;
    let mut plugins = Vec::with_capacity(rows.len());
    for row in rows {
        let name = row["name"]
            .as_str()
            .ok_or_else(|| error("plugin name missing"))?;
        if name.is_empty()
            || name.len() > 4096
            || name.bytes().any(|byte| b"/\\:\0".contains(&byte))
        {
            return Err(error("unsupported or unsafe plugin name in frozen census"));
        }
        let relative_path = format!("Data/{name}");
        let file = files
            .iter()
            .find(|candidate| candidate["path"] == relative_path)
            .ok_or_else(|| error("plugin has no source fingerprint in frozen census"))?;
        let sha256 = file["sha256"]
            .as_str()
            .ok_or_else(|| error("plugin source hash missing from frozen census"))?;
        let header_flags = row["header_flags"]
            .as_u64()
            .filter(|flags| *flags <= u64::from(u32::MAX))
            .ok_or_else(|| error("plugin header flags invalid"))? as u32;
        let master_rows = row["masters"]
            .as_array()
            .ok_or_else(|| error("plugin master table missing"))?;
        if master_rows.len() > MAX_MASTERS_PER_PLUGIN {
            return Err(error("plugin master table exceeds node budget"));
        }
        let masters = master_rows
            .iter()
            .map(|master| {
                let name = master
                    .as_str()
                    .ok_or_else(|| error("master name invalid"))?;
                if name.len() > 4096 {
                    return Err(error("master name exceeds byte budget"));
                }
                Ok(name.to_owned())
            })
            .collect::<Result<Vec<_>>>()?;
        plugins.push(Plugin {
            name: name.into(),
            sha256: sha256.into(),
            header_flags,
            masters,
        });
    }
    Ok(plugins)
}

fn graph_report(plugins: &[Plugin]) -> Value {
    let index = name_index(plugins);
    let mut statuses = BTreeMap::<String, u64>::new();
    let mut edges = 0u64;
    let nodes: Vec<_> = plugins
        .iter()
        .enumerate()
        .map(|(id, plugin)| {
            let masters: Vec<_> = plugin
                .masters
                .iter()
                .enumerate()
                .map(|(ordinal, name)| {
                    edges += 1;
                    let binding = bind_master(name, &index);
                    *statuses.entry(binding.status().into()).or_default() += 1;
                    json!({"master_table_index":ordinal,"raw_name":name,"binding":binding})
                })
                .collect();
            json!({
                "plugin_id":id,
                "name":plugin.name,
                "source_sha256":plugin.sha256,
                "header_flags":plugin.header_flags,
                "is_master_flagged":plugin.header_flags & 1 != 0,
                "is_localized_flagged":plugin.header_flags & 0x80 != 0,
                "is_small_master_flagged":formid::is_small_master_flag(plugin.header_flags),
                "declared_masters":masters
            })
        })
        .collect();
    let cycles = graph_cycles(plugins, &index);
    let duplicate_names: Vec<_> = index
        .iter()
        .filter(|(_, ids)| ids.len() > 1)
        .map(|(name, ids)| json!({"normalized_name":name,"plugins":ids}))
        .collect();
    json!({
        "schema":1,
        "status":"physical_plugin_master_dependency_graph",
        "plugin_count":plugins.len(),
        "declared_master_edges":edges,
        "edge_binding_statuses":statuses,
        "duplicate_physical_plugin_names":duplicate_names,
        "declared_master_cycles":cycles,
        "plugins":nodes,
        "limits":[
            "Uses physical TES4 master declarations only; it is not a plugin activation or load-order list",
            "Bindings identify physical filename candidates only; no ESL slot or FormKey is assigned",
            "No plugin/master/override winner is selected"
        ]
    })
}

fn checked_output(requested: &Path, proof: &Path) -> Result<std::path::PathBuf> {
    let parent = fs::canonicalize(requested.parent().unwrap_or(Path::new(".")))?;
    let local = fs::canonicalize("local")?;
    if !parent.starts_with(&local) || parent.starts_with(proof) {
        return Err(error(
            "new graph output must be in local/, outside the input proof",
        ));
    }
    Ok(parent.join(
        requested
            .file_name()
            .ok_or_else(|| error("output directory name missing"))?,
    ))
}

fn run() -> Result<()> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(error(
            "audit-plugin-deps <completed-content-proof> <new-local-output>",
        ));
    }
    let proof = fs::canonicalize(&args[0])?;
    let corpus_path = proof.join("census.json");
    let complete_path = proof.join("complete.json");
    if fs::metadata(&corpus_path)?.len() > MAX_CENSUS_BYTES {
        return Err(error("content census exceeds byte budget"));
    }
    let complete: Value = serde_json::from_reader(File::open(&complete_path)?)?;
    let census_sha256 = census::hash_file(&corpus_path)?;
    if complete["status"] != "matched" || complete["census_sha256"] != census_sha256 {
        return Err(error("completed content proof required"));
    }
    let corpus: Value = serde_json::from_reader(File::open(&corpus_path)?)?;
    let plugins = load_plugins(&corpus)?;
    let graph = graph_report(&plugins);
    let output = checked_output(Path::new(&args[1]), &proof)?;
    fs::create_dir(&output)?;
    let graph_path = output.join("graph.json");
    serde_json::to_writer_pretty(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&graph_path)?,
        &graph,
    )?;
    let result = json!({
        "schema":1,
        "status":"physical_plugin_master_dependency_audit",
        "source_census_sha256":census_sha256,
        "source_proof_complete_sha256":census::hash_file(&complete_path)?,
        "plugins":plugins.len(),
        "declared_master_edges":graph["declared_master_edges"],
        "edge_binding_statuses":graph["edge_binding_statuses"],
        "duplicate_physical_plugin_names":graph["duplicate_physical_plugin_names"],
        "declared_master_cycles":graph["declared_master_cycles"],
        "graph_sha256":census::hash_file(&graph_path)?,
        "limits":graph["limits"]
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
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn plugin(name: &str, masters: &[&str]) -> Plugin {
        Plugin {
            name: name.into(),
            sha256: "fixture".into(),
            header_flags: 0,
            masters: masters.iter().map(|name| (*name).into()).collect(),
        }
    }

    #[test]
    fn physical_dependency_names_are_case_insensitive_but_never_winners() {
        let plugins = vec![
            plugin("Base.esm", &[]),
            plugin("Patch.esp", &["base.ESM", "Absent.esm"]),
            plugin("BASE.ESM", &[]),
        ];
        let index = name_index(&plugins);
        assert_eq!(
            bind_master("base.esm", &index),
            MasterBinding::Ambiguous {
                name: "base.esm".into(),
                plugins: vec![0, 2]
            }
        );
        assert_eq!(
            bind_master("Absent.esm", &index),
            MasterBinding::Missing {
                name: "Absent.esm".into()
            }
        );
        assert!(matches!(
            bind_master("\u{e9}.esm", &index),
            MasterBinding::UnsupportedIdentifier { .. }
        ));
    }

    #[test]
    fn declared_master_cycles_are_reported_without_ordering_plugins() {
        let plugins = vec![
            plugin("A.esm", &["B.esm"]),
            plugin("B.esm", &["A.esm"]),
            plugin("C.esm", &["A.esm"]),
        ];
        let index = name_index(&plugins);
        assert_eq!(graph_cycles(&plugins, &index), vec![vec![0, 1]]);
    }

    #[test]
    fn output_is_new_local_and_outside_input_proof() {
        let proof = tempdir().unwrap();
        let local = fs::canonicalize("local").unwrap();
        let requested = local.join("plugin-deps-test");
        assert_eq!(checked_output(&requested, proof.path()).unwrap(), requested);
        assert!(checked_output(proof.path(), proof.path()).is_err());
    }
}
