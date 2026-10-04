//! Bounded physical source relations. Raw OR bits grant no evaluation authority.
use super::{OwnerStatus, PreparedOwnerRecord, RecordIdentity, record};
use crate::{Error, Result};
use serde::Serialize;

const BYTE_BUDGET: &str = "condition source run metadata byte budget exceeded";

#[derive(Debug, Clone, Copy)]
pub struct RunLimits {
    pub maximum_runs: usize,
    pub maximum_retained_bytes: usize,
}
impl Default for RunLimits {
    fn default() -> Self {
        Self {
            maximum_runs: 1_000_000,
            maximum_retained_bytes: 128 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEnd {
    RawOrClear,
    RecordEnd,
    UnownedNextSite,
    OwnerChange,
    PhysicalFieldGap,
}

#[derive(Debug, Serialize)]
pub struct SourceRun {
    pub owner_section: usize,
    pub first_site: usize,
    pub end_site_exclusive: usize,
    pub tail_or_flag: bool,
    pub end_reason: RunEnd,
}

#[derive(Debug, Serialize)]
pub struct SourceRuns<'a> {
    identity: &'a RecordIdentity,
    runs: Vec<SourceRun>,
    orphan_sites: usize,
    unmapped_sites: usize,
    evaluation_ready: bool,
    group_evaluation_verified: bool,
    default_subjects_applied: bool,
    #[serde(skip)]
    retained_bytes: usize,
}
impl SourceRuns<'_> {
    pub fn identity(&self) -> &RecordIdentity {
        self.identity
    }
    pub fn runs(&self) -> &[SourceRun] {
        &self.runs
    }
    pub fn orphan_sites(&self) -> usize {
        self.orphan_sites
    }
    pub fn unmapped_sites(&self) -> usize {
        self.unmapped_sites
    }
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

pub fn prepare_source_runs(
    prepared: &PreparedOwnerRecord,
    limits: RunLimits,
) -> Result<SourceRuns<'_>> {
    let sites = prepared.conditions().sites();
    let owners = prepared.ownership();
    if sites.len() != owners.sites().len() {
        return Err(Error::Resolution(
            "condition owner/site count mismatch".into(),
        ));
    }
    let unowned = owners
        .sites()
        .iter()
        .filter(|site| site.owner_section.is_none())
        .count();
    let mapped = owners.status() == OwnerStatus::MappedNarrativeSource;
    let mut result = SourceRuns {
        identity: prepared.conditions().identity(),
        runs: Vec::new(),
        orphan_sites: if mapped { unowned } else { 0 },
        unmapped_sites: if mapped { 0 } else { unowned },
        evaluation_ready: false,
        group_evaluation_verified: false,
        default_subjects_applied: false,
        retained_bytes: 0,
    };
    let mut used =
        record::admitted_json_bytes(&result, limits.maximum_retained_bytes, BYTE_BUDGET)?;
    let mut first = None;
    for (index, site) in sites.iter().enumerate() {
        let Some(owner) = owners.sites()[index].owner_section else {
            debug_assert!(first.is_none());
            continue;
        };
        let first_site = *first.get_or_insert(index);
        let tail_or_flag = site.condition().or_flag();
        let end_reason = if !tail_or_flag {
            Some(RunEnd::RawOrClear)
        } else if index + 1 == sites.len() {
            Some(RunEnd::RecordEnd)
        } else {
            let next = &sites[index + 1];
            match owners.sites()[index + 1].owner_section {
                None => Some(RunEnd::UnownedNextSite),
                Some(next_owner) if next_owner != owner => Some(RunEnd::OwnerChange),
                Some(_)
                    if next.preceding_field_kind() != Some("CTDA")
                        || next.preceding_field_decoded_offset()
                            != Some(site.field_decoded_offset())
                        || site
                            .field_decoded_offset()
                            .checked_add(6 + site.raw_bytes().len())
                            != Some(next.field_decoded_offset()) =>
                {
                    Some(RunEnd::PhysicalFieldGap)
                }
                Some(_) => None,
            }
        };
        let Some(end_reason) = end_reason else {
            continue;
        };
        if result.runs.len() >= limits.maximum_runs {
            return Err(Error::Unsupported(
                "condition source run count budget exceeded".into(),
            ));
        }
        let row = SourceRun {
            owner_section: owner,
            first_site,
            end_site_exclusive: index + 1,
            tail_or_flag,
            end_reason,
        };
        let additional = record::admitted_json_bytes(
            &row,
            limits.maximum_retained_bytes.saturating_sub(used),
            BYTE_BUDGET,
        )? + usize::from(!result.runs.is_empty());
        if additional > limits.maximum_retained_bytes.saturating_sub(used) {
            return Err(Error::Unsupported(BYTE_BUDGET.into()));
        }
        used += additional;
        result.runs.push(row);
        first = None;
    }
    let exact = record::admitted_json_bytes(&result, limits.maximum_retained_bytes, BYTE_BUDGET)?;
    if exact != used {
        return Err(Error::Resolution(
            "condition source run admission accounting mismatch".into(),
        ));
    }
    result.retained_bytes = exact;
    Ok(result)
}
