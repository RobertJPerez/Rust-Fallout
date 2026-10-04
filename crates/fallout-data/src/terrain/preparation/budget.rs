use crate::{Error, Result, identity::FormKey, plugin, store::RecordStore};
use serde::Serialize;

/// Lowerable source-only ceilings; logical metadata excludes mappings/allocator overhead.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Limits {
    pub winners: usize,
    pub sources: usize,
    pub worlds: usize,
    pub records: usize,
    pub layers: usize,
    pub assets: usize,
    pub candidates: usize,
    pub archives: usize,
    pub record_bytes: usize,
    pub plugin_bytes: usize,
    pub field_sites: usize,
    pub path_bytes: usize,
    pub texture_bytes: usize,
    pub metadata_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            winners: 1_000_000,
            sources: 256,
            worlds: 256,
            records: 128,
            layers: 4096,
            assets: 256,
            candidates: 4096,
            archives: 8,
            record_bytes: 1024 * 1024,
            plugin_bytes: 8 * 1024 * 1024,
            field_sites: 262144,
            path_bytes: 4096,
            texture_bytes: 64 * 1024 * 1024,
            metadata_bytes: 32 * 1024 * 1024,
        }
    }
}
impl Limits {
    pub(super) fn validate(self) -> Result<Self> {
        let cap = Self::default();
        for (value, maximum, name) in [
            (self.winners, cap.winners, "winners"),
            (self.sources, cap.sources, "sources"),
            (self.worlds, cap.worlds, "worlds"),
            (self.records, cap.records, "records"),
            (self.layers, cap.layers, "layers"),
            (self.assets, cap.assets, "assets"),
            (self.candidates, cap.candidates, "candidates"),
            (self.archives, cap.archives, "archives"),
            (self.record_bytes, cap.record_bytes, "record bytes"),
            (self.plugin_bytes, cap.plugin_bytes, "plugin bytes"),
            (self.field_sites, cap.field_sites, "field sites"),
            (self.path_bytes, cap.path_bytes, "path bytes"),
            (self.texture_bytes, cap.texture_bytes, "texture bytes"),
            (self.metadata_bytes, cap.metadata_bytes, "metadata bytes"),
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
    pub plugin_bytes: usize,
    pub field_sites: usize,
    pub layers: usize,
    pub candidates: usize,
    pub archives: usize,
    pub texture_bytes: usize,
    pub metadata_bytes: usize,
}
pub(in crate::terrain) struct Budget {
    pub limits: Limits,
    pub usage: Usage,
}
impl Budget {
    pub fn new(limits: Limits, store: &RecordStore) -> Result<Self> {
        let limits = limits.validate()?;
        if store.winners.len() > limits.winners {
            return Err(failure("winners", "exceeded"));
        }
        if store.indices().len() > limits.sources {
            return Err(failure("sources", "exceeded"));
        }
        let mut result = Self {
            limits,
            usage: Usage::default(),
        };
        for source in store.indices() {
            result.metadata(512 + 2 * source.census.name.len())?;
        }
        Ok(result)
    }
    pub fn metadata(&mut self, bytes: usize) -> Result<()> {
        charge(
            &mut self.usage.metadata_bytes,
            bytes,
            self.limits.metadata_bytes,
            "metadata bytes",
        )
    }
    pub fn entry(&mut self, key: &FormKey, name: &str) -> Result<()> {
        self.metadata(1024 + 4 * key.origin_plugin.len() + 4 * name.len())
    }
    pub fn maximum(&self, existing: usize) -> usize {
        existing
            .min(self.limits.record_bytes)
            .min(self.limits.plugin_bytes - self.usage.plugin_bytes)
    }
    pub fn body(&mut self, record: &plugin::Record, name: &str) -> Result<()> {
        charge(
            &mut self.usage.plugin_bytes,
            record.payload.len(),
            self.limits.plugin_bytes,
            "plugin bytes",
        )?;
        // Typed retained fields, raw unknown bytes, temporary normalization and
        // map/link overhead are reserved before the existing decoder allocates.
        self.metadata(
            record
                .payload
                .len()
                .checked_mul(4)
                .ok_or_else(|| failure("metadata bytes", "overflow"))?,
        )?;
        plugin::visit_subrecords(record, name, |field| {
            charge(
                &mut self.usage.field_sites,
                1,
                self.limits.field_sites,
                "field sites",
            )?;
            self.metadata(512)?;
            if matches!(&field.kind, b"BTXT" | b"ATXT") {
                charge(&mut self.usage.layers, 1, self.limits.layers, "layers")?;
                self.metadata(1024)?;
            }
            if matches!(
                &field.kind,
                b"TX00" | b"TX01" | b"TX02" | b"TX03" | b"TX04" | b"TX05"
            ) {
                // Account for optional textures/ prefix before typed/path clones.
                if field.data.len().saturating_sub(1) + 9 > self.limits.path_bytes {
                    return Err(failure("path bytes", "exceeded"));
                }
                self.metadata(8 * field.data.len())?;
            }
            Ok(())
        })
    }
}
pub(in crate::terrain) fn failure(name: &str, reason: &str) -> Error {
    Error::Resolution(format!("terrain preparation {name} budget: {reason}"))
}
pub(in crate::terrain) fn charge(
    current: &mut usize,
    add: usize,
    maximum: usize,
    name: &str,
) -> Result<()> {
    let next = current
        .checked_add(add)
        .ok_or_else(|| failure(name, "overflow"))?;
    if next > maximum {
        return Err(failure(name, "exceeded"));
    }
    *current = next;
    Ok(())
}
