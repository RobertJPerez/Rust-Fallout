use crate::{Error, Result};
use serde::Serialize;

/// Source-inspection ceilings. Callers may lower each value, never raise it.
/// Retained metadata is a conservative logical estimate, not process peak memory.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Limits {
    pub max_winners_scanned: usize,
    pub max_sources: usize,
    pub max_nodes: usize,
    pub max_edges: usize,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_field_sites: usize,
    pub max_metadata_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_winners_scanned: 1_000_000,
            max_sources: 256,
            max_nodes: 4096,
            max_edges: 16384,
            max_record_bytes: 4 * 1024 * 1024,
            max_decoded_bytes: 64 * 1024 * 1024,
            max_field_sites: 262144,
            max_metadata_bytes: 32 * 1024 * 1024,
        }
    }
}

impl Limits {
    pub(in crate::world) fn validate(self) -> Result<Self> {
        let ceiling = Self::default();
        for (value, maximum, name) in [
            (
                self.max_winners_scanned,
                ceiling.max_winners_scanned,
                "winning scan",
            ),
            (self.max_sources, ceiling.max_sources, "sources"),
            (self.max_nodes, ceiling.max_nodes, "nodes"),
            (self.max_edges, ceiling.max_edges, "edges"),
            (
                self.max_record_bytes,
                ceiling.max_record_bytes,
                "record bytes",
            ),
            (
                self.max_decoded_bytes,
                ceiling.max_decoded_bytes,
                "decoded bytes",
            ),
            (self.max_field_sites, ceiling.max_field_sites, "field sites"),
            (
                self.max_metadata_bytes,
                ceiling.max_metadata_bytes,
                "metadata bytes",
            ),
        ] {
            if value > maximum {
                return Err(failure(name, "ceiling exceeded"));
            }
        }
        Ok(self)
    }
}

#[derive(Debug, Default, Serialize)]
pub struct Usage {
    pub winners_scanned: usize,
    pub sources: usize,
    pub nodes: usize,
    pub edges: usize,
    pub decoded_bytes: usize,
    pub field_sites: usize,
    pub metadata_bytes: usize,
}

pub(super) struct Budget {
    pub limits: Limits,
    pub usage: Usage,
}

impl Budget {
    pub fn new(limits: Limits) -> Result<Self> {
        Ok(Self {
            limits: limits.validate()?,
            usage: Usage::default(),
        })
    }

    pub fn metadata(&mut self, bytes: usize) -> Result<()> {
        charge(
            &mut self.usage.metadata_bytes,
            bytes,
            self.limits.max_metadata_bytes,
            "metadata bytes",
        )
    }

    pub fn node(&mut self, bytes: usize) -> Result<()> {
        charge(&mut self.usage.nodes, 1, self.limits.max_nodes, "nodes")?;
        self.metadata(bytes)
    }

    pub fn edge(&mut self, bytes: usize) -> Result<()> {
        charge(&mut self.usage.edges, 1, self.limits.max_edges, "edges")?;
        self.metadata(bytes)
    }

    pub fn field(&mut self, bytes: usize) -> Result<()> {
        charge(
            &mut self.usage.field_sites,
            1,
            self.limits.max_field_sites,
            "field sites",
        )?;
        self.metadata(bytes)
    }

    pub fn read_maximum(&self) -> usize {
        self.limits
            .max_record_bytes
            .min(self.limits.max_decoded_bytes - self.usage.decoded_bytes)
    }

    pub fn decoded(&mut self, bytes: usize) -> Result<()> {
        charge(
            &mut self.usage.decoded_bytes,
            bytes,
            self.limits.max_decoded_bytes,
            "decoded bytes",
        )
    }
}

pub(super) fn failure(name: &str, reason: &str) -> Error {
    Error::Resolution(format!("world dependency {name} budget: {reason}"))
}

pub(super) fn charge(current: &mut usize, add: usize, maximum: usize, name: &str) -> Result<()> {
    let value = current
        .checked_add(add)
        .ok_or_else(|| failure(name, "overflow"))?;
    if value > maximum {
        return Err(failure(name, "exceeded"));
    }
    *current = value;
    Ok(())
}
