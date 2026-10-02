//! A dry run for source preparation. Jobs describe existing readers; a plan is
//! never a claim that meshes, scripts, or gameplay have been converted.
use crate::{
    Error, Result,
    baseline::{Baseline, Fingerprint},
    content::PluginIndex,
    identity::{ProfileId, plugin_name},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
pub struct ImportPlan {
    pub schema_version: u32,
    pub profile: ProfileId,
    pub content_fingerprint: String,
    pub explicit_load_order: Vec<String>,
    pub jobs: Vec<PreparationJob>,
    pub unsupported: Vec<UnsupportedInput>,
    pub untouched_non_data_files: usize,
    pub integrity_failures: usize,
    pub missing_required: Vec<String>,
    pub runtime_ready: bool,
    pub remaining: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct PreparationJob {
    pub key: String,
    pub operation: &'static str,
    pub transform_version: &'static str,
    pub source: Fingerprint,
    pub dependencies: Vec<Fingerprint>,
}

#[derive(Debug, Serialize)]
pub struct UnsupportedInput {
    pub source: Fingerprint,
    pub reason: &'static str,
}

pub fn dry_run(baseline: &Baseline, indices: &[PluginIndex]) -> Result<ImportPlan> {
    let sources: BTreeMap<_, _> = baseline
        .files
        .iter()
        .map(|f| (f.path.to_ascii_lowercase(), f))
        .collect();
    if sources.len() != baseline.files.len() {
        return Err(Error::Resolution(
            "duplicate source path in baseline".into(),
        ));
    }
    let mut plugins = BTreeMap::new();
    for index in indices {
        let name = plugin_name(&index.census.name)?;
        if plugins.insert(format!("data/{name}"), index).is_some() {
            return Err(Error::Resolution("duplicate plugin in import plan".into()));
        }
    }
    let mut jobs = Vec::new();
    let mut unsupported = Vec::new();
    let mut untouched = 0;
    for (path, source) in &sources {
        let Some(relative) = path.strip_prefix("data/") else {
            untouched += 1;
            continue;
        };
        let extension = relative.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        if let Some(index) = plugins.get(path) {
            let mut dependencies = Vec::new();
            // Master order is meaningful. Keep it even though jobs themselves are
            // sorted by path and independent of discovery/worker completion order.
            for master in &index.census.masters {
                let key = format!("data/{}", plugin_name(master)?);
                dependencies.push(
                    (*sources.get(&key).ok_or_else(|| {
                        Error::Resolution(format!("import plan is missing master {master}"))
                    })?)
                    .clone(),
                );
            }
            jobs.push(job(
                "index-nv-plugin",
                "nv-plugin-index-v1",
                source,
                dependencies,
            )?);
        } else if extension == "bsa" && !relative.contains('/') {
            jobs.push(job("index-nv-bsa", "nv-bsa-index-v1", source, vec![])?);
        } else {
            unsupported.push(UnsupportedInput {
                source: (*source).clone(),
                reason: if ["esm", "esp"].contains(&extension) {
                    "plugin is outside the explicit load order; no import scheduled"
                } else {
                    "loose asset conversion and game lookup precedence are not implemented"
                },
            });
        }
    }
    for path in plugins.keys() {
        if !sources.contains_key(path) {
            return Err(Error::Resolution(format!(
                "plugin absent from baseline: {path}"
            )));
        }
    }
    Ok(ImportPlan {
        schema_version: 1,
        profile: ProfileId::NvOriginal,
        content_fingerprint: baseline.content_fingerprint.clone(),
        explicit_load_order: indices.iter().map(|p| p.census.name.clone()).collect(),
        jobs,
        unsupported,
        untouched_non_data_files: untouched,
        integrity_failures: indices
            .iter()
            .map(|p| p.census.integrity_issues.len())
            .sum(),
        missing_required: baseline.missing_required.clone(),
        runtime_ready: false,
        remaining: vec![
            "jobs are source preparation only; no canonical world is emitted",
            "archive members still need typed conversion and asset dependency plans",
            "archive/loose precedence and record-specific merge rules are unverified",
            "job execution graph, cancellation, and conversion journal are not implemented",
        ],
    })
}

fn job(
    operation: &'static str,
    transform_version: &'static str,
    source: &Fingerprint,
    dependencies: Vec<Fingerprint>,
) -> Result<PreparationJob> {
    let identity = serde_json::to_vec(&(
        ProfileId::NvOriginal,
        operation,
        transform_version,
        source,
        &dependencies,
    ))
    .map_err(|e| Error::Resolution(e.to_string()))?;
    let mut digest = Sha256::new();
    digest.update(b"fallout-preparation-job-v1\0");
    digest.update(identity);
    Ok(PreparationJob {
        key: format!("{:x}", digest.finalize()),
        operation,
        transform_version,
        source: source.clone(),
        dependencies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source(path: &str, hash: char) -> Fingerprint {
        Fingerprint {
            path: path.into(),
            bytes: 16,
            sha256: hash.to_string().repeat(64),
        }
    }
    #[test]
    fn enumeration_order_does_not_change_the_plan() {
        let mut baseline = Baseline {
            schema_version: 1,
            profile: "nv-original",
            installation: "ignored-absolute-location".into(),
            content_fingerprint: "a".repeat(64),
            files: vec![source("Data/B.bsa", 'b'), source("Data/A.bsa", 'a')],
            missing_required: vec![],
            unverified: vec![],
        };
        let first = serde_json::to_vec(&dry_run(&baseline, &[]).unwrap()).unwrap();
        baseline.files.reverse();
        baseline.installation = "moved-installation".into();
        assert_eq!(
            first,
            serde_json::to_vec(&dry_run(&baseline, &[]).unwrap()).unwrap()
        );
    }
    #[test]
    fn master_changes_invalidate_only_dependent_jobs() {
        let src = source("Data/Patch.esp", 'a');
        let master = source("Data/Base.esm", 'b');
        let before = job("index-nv-plugin", "v1", &src, vec![master]).unwrap();
        let after = job(
            "index-nv-plugin",
            "v1",
            &src,
            vec![source("Data/Base.esm", 'c')],
        )
        .unwrap();
        assert_ne!(before.key, after.key);
        let archive = job("index-nv-bsa", "v1", &src, vec![]).unwrap();
        assert_eq!(
            archive.key,
            job("index-nv-bsa", "v1", &src, vec![]).unwrap().key
        );
        assert_ne!(
            archive.key,
            job("index-nv-bsa", "v2", &src, vec![]).unwrap().key
        );
    }
}
