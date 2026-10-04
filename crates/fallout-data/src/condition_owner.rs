//! Physical source containers, not executable condition grouping or truth.
use super::{PreparedRecord, RecordLimits, Signatures, record};
use crate::{
    Error, Result, narrative, narrative_census,
    store::{Location, RecordStore},
};
use serde::Serialize;
use std::collections::BTreeMap;

const OWNER_BUDGET: &str = "condition owner metadata byte budget exceeded";

#[derive(Debug, Clone, Copy)]
pub struct OwnerLimits {
    pub maximum_sections: usize,
    pub maximum_findings: usize,
    pub maximum_retained_bytes: usize,
}
impl Default for OwnerLimits {
    fn default() -> Self {
        Self {
            maximum_sections: 65_536,
            maximum_findings: 65_536,
            maximum_retained_bytes: 128 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerStatus {
    MappedNarrativeSource,
    UnmappedRecordKind,
}

#[derive(Debug, Serialize)]
pub struct OwnerSite {
    pub site_index: usize,
    pub field_decoded_offset: usize,
    pub owner_section: Option<usize>,
}
#[derive(Debug, Serialize)]
pub struct SourceList {
    pub owner_section: usize,
    pub site_indices: Vec<usize>,
}
#[derive(Debug, Serialize)]
pub struct SourceOwners {
    status: OwnerStatus,
    sections: Vec<narrative::Section>,
    sites: Vec<OwnerSite>,
    source_lists: Vec<SourceList>,
    findings: Vec<narrative::Finding>,
    narrative_fields_sha256: Option<String>,
    evaluation_ready: bool,
    group_evaluation_verified: bool,
    default_subjects_applied: bool,
    #[serde(skip)]
    retained_bytes: usize,
}
impl SourceOwners {
    pub fn status(&self) -> OwnerStatus {
        self.status
    }
    pub fn sections(&self) -> &[narrative::Section] {
        &self.sections
    }
    pub fn sites(&self) -> &[OwnerSite] {
        &self.sites
    }
    pub fn source_lists(&self) -> &[SourceList] {
        &self.source_lists
    }
    pub fn findings(&self) -> &[narrative::Finding] {
        &self.findings
    }
    pub fn narrative_fields_sha256(&self) -> Option<&str> {
        self.narrative_fields_sha256.as_deref()
    }
    pub fn retained_bytes(&self) -> usize {
        self.retained_bytes
    }
}

#[derive(Debug, Serialize)]
pub struct PreparedOwnerRecord {
    conditions: PreparedRecord,
    ownership: SourceOwners,
}
impl PreparedOwnerRecord {
    pub fn conditions(&self) -> &PreparedRecord {
        &self.conditions
    }
    pub fn ownership(&self) -> &SourceOwners {
        &self.ownership
    }
}

pub fn prepare_record_with_owners(
    store: &mut RecordStore,
    location: Location,
    signatures: &Signatures,
    limits: RecordLimits,
    owner_limits: OwnerLimits,
) -> Result<PreparedOwnerRecord> {
    let (conditions, body) = record::prepare_record_body(store, location, signatures, limits)?;
    let mapped = matches!(&body.header.kind, b"QUST" | b"INFO");
    let document = if mapped {
        Some(narrative::decode(
            &body,
            &conditions.identity().source_name,
            narrative::Limits {
                max_fields: limits.maximum_fields,
                max_sections: owner_limits.maximum_sections,
                max_findings: owner_limits.maximum_findings,
            },
        )?)
    } else {
        None
    };
    let digest = document.as_ref().map(narrative_census::fields_digest);
    // Admit borrowed metadata before transferring it into the returned view.
    if let Some(document) = &document {
        record::admitted_json_bytes(
            &(&document.sections, &document.findings),
            owner_limits.maximum_retained_bytes,
            OWNER_BUDGET,
        )?;
    }
    let (sections, findings, fields) = document.map_or_else(
        || (Vec::new(), Vec::new(), Vec::new()),
        |document| (document.sections, document.findings, document.fields),
    );
    let mut owner_fields = fields.iter().filter(|field| field.kind == *b"CTDA");
    let mut owners = SourceOwners {
        status: if mapped {
            OwnerStatus::MappedNarrativeSource
        } else {
            OwnerStatus::UnmappedRecordKind
        },
        sections,
        sites: Vec::new(),
        source_lists: Vec::new(),
        findings,
        narrative_fields_sha256: digest,
        evaluation_ready: false,
        group_evaluation_verified: false,
        default_subjects_applied: false,
        retained_bytes: 0,
    };
    let mut used =
        record::admitted_json_bytes(&owners, owner_limits.maximum_retained_bytes, OWNER_BUDGET)?;
    let mut list_by_owner = BTreeMap::new();
    for (site_index, site) in conditions.sites().iter().enumerate() {
        let owner_section = if mapped {
            let field = owner_fields
                .next()
                .ok_or_else(|| Error::Resolution("condition owner/site count mismatch".into()))?;
            if field.offset != site.field_decoded_offset() || field.data != site.raw_bytes() {
                return Err(Error::Resolution(
                    "condition owner/source bytes mismatch".into(),
                ));
            }
            field.owner
        } else {
            None
        };
        let row = OwnerSite {
            site_index,
            field_decoded_offset: site.field_decoded_offset(),
            owner_section,
        };
        let row_bytes = record::admitted_json_bytes(
            &row,
            owner_limits.maximum_retained_bytes.saturating_sub(used),
            OWNER_BUDGET,
        )?;
        let mut additional = row_bytes + usize::from(!owners.sites.is_empty());
        if let Some(section) = owner_section {
            if section >= owners.sections.len() {
                return Err(Error::Resolution(
                    "condition owner section is outside source table".into(),
                ));
            }
            let list_index = if let Some(index) = list_by_owner.get(&section).copied() {
                index
            } else {
                let list = SourceList {
                    owner_section: section,
                    site_indices: Vec::new(),
                };
                additional += record::admitted_json_bytes(
                    &list,
                    owner_limits
                        .maximum_retained_bytes
                        .saturating_sub(used + additional),
                    OWNER_BUDGET,
                )?;
                additional += usize::from(!owners.source_lists.is_empty());
                if additional > owner_limits.maximum_retained_bytes.saturating_sub(used) {
                    return Err(Error::Unsupported(OWNER_BUDGET.into()));
                }
                let index = owners.source_lists.len();
                owners.source_lists.push(list);
                list_by_owner.insert(section, index);
                index
            };
            let list = &mut owners.source_lists[list_index];
            additional += record::admitted_json_bytes(
                &site_index,
                owner_limits
                    .maximum_retained_bytes
                    .saturating_sub(used + additional),
                OWNER_BUDGET,
            )?;
            additional += usize::from(!list.site_indices.is_empty());
            if additional > owner_limits.maximum_retained_bytes.saturating_sub(used) {
                return Err(Error::Unsupported(OWNER_BUDGET.into()));
            }
            list.site_indices.push(site_index);
        }
        if additional > owner_limits.maximum_retained_bytes.saturating_sub(used) {
            return Err(Error::Unsupported(OWNER_BUDGET.into()));
        }
        used += additional;
        owners.sites.push(row);
    }
    if owner_fields.next().is_some() {
        return Err(Error::Resolution(
            "condition owner/site count mismatch".into(),
        ));
    }
    let exact =
        record::admitted_json_bytes(&owners, owner_limits.maximum_retained_bytes, OWNER_BUDGET)?;
    if exact != used {
        return Err(Error::Resolution(
            "condition owner admission accounting mismatch".into(),
        ));
    }
    owners.retained_bytes = exact;
    Ok(PreparedOwnerRecord {
        conditions,
        ownership: owners,
    })
}
