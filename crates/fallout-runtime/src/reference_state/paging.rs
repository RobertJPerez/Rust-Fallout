//! Bounded observations for canonical scene reconciliation. Filtering uses only
//! explicit stored cells; missing components never acquire source defaults.
use super::{State, View};
use crate::{
    Error, Result, World,
    identity::{CampaignId, ReferenceId, valid_form},
};
use fallout_data::identity::FormKey;
use std::{mem::size_of, ops::Bound};

/// None selects the whole reference registry, including unavailable state.
/// Some(cell) selects only explicit components naming that exact source key.
#[derive(Debug, Clone, Copy, Default)]
pub struct PageRequest<'a> {
    pub cell: Option<&'a FormKey>,
    pub after: Option<&'a Cursor>,
}

#[derive(Debug, Clone, Copy)]
pub struct PageLimits {
    pub max_visited: usize,
    pub max_rows: usize,
    /// Conservative fixed-value and owned UTF-8 charge, including possible
    /// continuation metadata. This is not an allocator/process memory ceiling.
    pub max_copied_bytes: usize,
}
impl Default for PageLimits {
    fn default() -> Self {
        Self {
            max_visited: 256,
            max_rows: 64,
            max_copied_bytes: 64 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageUsage {
    /// References whose component/filter was inspected. A byte-limited candidate
    /// may be inspected again on the next page; it has not been consumed.
    pub visited: usize,
    pub returned: usize,
    pub charged_bytes: usize,
}

/// Only this producer constructs continuation positions. It cannot deserialize
/// as authority, and is never a save field or a render entity identifier.
#[derive(Debug, Clone)]
pub struct Cursor {
    epoch: u64,
    campaign: CampaignId,
    catalogue_sha256: String,
    revision: u64,
    cell: Option<FormKey>,
    consumed: ReferenceId,
}

#[derive(Debug)]
pub struct Page {
    campaign: CampaignId,
    catalogue_sha256: String,
    revision: u64,
    cell: Option<FormKey>,
    rows: Vec<View>,
    cursor: Option<Cursor>,
    usage: PageUsage,
}
impl Page {
    pub fn campaign(&self) -> CampaignId {
        self.campaign
    }
    pub fn catalogue_fingerprint(&self) -> &str {
        &self.catalogue_sha256
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn cell(&self) -> Option<&FormKey> {
        self.cell.as_ref()
    }
    pub fn rows(&self) -> &[View] {
        &self.rows
    }
    pub fn next_cursor(&self) -> Option<&Cursor> {
        self.cursor.as_ref()
    }
    pub fn usage(&self) -> PageUsage {
        self.usage
    }
    pub fn into_parts(self) -> (Vec<View>, Option<Cursor>) {
        (self.rows, self.cursor)
    }
}

fn copy_charge(parts: &[usize]) -> Result<usize> {
    parts
        .iter()
        .try_fold(0usize, |sum, part| sum.checked_add(*part))
        .ok_or(Error::Capacity("reference page copied bytes"))
}

impl World<'_> {
    /// Bounds registry traversal independently of matches, so even a zero-match
    /// page can return a useful continuation. No whole-world snapshot is made.
    pub fn reference_state_page(
        &self,
        request: PageRequest<'_>,
        limits: PageLimits,
    ) -> Result<Page> {
        if let Some(cursor) = request.after {
            if cursor.epoch != self.epoch {
                return Err(Error::StaleHandle);
            }
            if cursor.campaign != self.campaign || cursor.catalogue_sha256 != self.cohort {
                return Err(Error::DefinitionChanged);
            }
            if cursor.revision != self.revision {
                return Err(Error::Invalid("reference page revision changed".into()));
            }
            if cursor.cell.as_ref() != request.cell {
                return Err(Error::Invalid("reference page filter changed".into()));
            }
        }
        if let Some(cell) = request.cell {
            valid_form(cell)?;
        }
        if limits.max_visited == 0 || limits.max_rows == 0 {
            return Err(Error::Capacity("reference page visited/row bounds"));
        }
        let start = request
            .after
            .map_or(Bound::Unbounded, |cursor| Bound::Excluded(cursor.consumed));
        let mut scan = self.references.range((start, Bound::Unbounded)).peekable();
        let filter_bytes = request.cell.map_or(0, |cell| cell.origin_plugin.len());
        let metadata = copy_charge(&[size_of::<Page>(), self.cohort.len(), filter_bytes])?;
        // Reserve the cursor's strings before traversing; a matching candidate
        // can exhaust the page even if most preceding references did not match.
        let continuation = if scan.peek().is_some() {
            copy_charge(&[self.cohort.len(), filter_bytes])?
        } else {
            0
        };
        let mut charged_bytes = copy_charge(&[metadata, continuation])?;
        if charged_bytes > limits.max_copied_bytes {
            return Err(Error::Capacity("reference page copied bytes"));
        }
        let mut rows = Vec::new();
        let mut visited = 0;
        let mut consumed = None;
        while visited < limits.max_visited && rows.len() < limits.max_rows {
            let Some((&reference, authored)) = scan.peek().copied() else {
                break;
            };
            visited += 1;
            let state = self.reference_states.get(&reference);
            if request
                .cell
                .is_none_or(|cell| state.is_some_and(|state| state.cell() == cell))
            {
                let charge = copy_charge(&[
                    size_of::<View>(),
                    self.cohort.len(),
                    authored.as_ref().map_or(0, |key| key.origin_plugin.len()),
                    state.map_or(0, |state: &State| state.cell().origin_plugin.len()),
                ])?;
                let next_charge = copy_charge(&[charged_bytes, charge])?;
                if next_charge > limits.max_copied_bytes {
                    // Resume before this candidate. Never return a continuation
                    // that quietly loses a matching reference due to its size.
                    if consumed.is_none() {
                        return Err(Error::Capacity("reference page copied bytes"));
                    }
                    break;
                }
                rows.push(self.reference_view(reference)?);
                charged_bytes = next_charge;
            }
            scan.next();
            consumed = Some(reference);
        }
        let cursor = if scan.peek().is_some() {
            Some(Cursor {
                epoch: self.epoch,
                campaign: self.campaign,
                catalogue_sha256: self.cohort.clone(),
                revision: self.revision,
                cell: request.cell.cloned(),
                consumed: consumed.expect("positive limits ensured page progress"),
            })
        } else {
            None
        };
        let usage = PageUsage {
            visited,
            returned: rows.len(),
            charged_bytes,
        };
        Ok(Page {
            campaign: self.campaign,
            catalogue_sha256: self.cohort.clone(),
            revision: self.revision,
            cell: request.cell.cloned(),
            rows,
            cursor,
            usage,
        })
    }
}
