//! Named engineering strip conversion; sealed source streams are the authority.
use super::{Budget, Identity, SourceSpan, Streams, Topology};
use crate::Result;
use serde::Serialize;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Policy {
    AlternatingStripWindingSkipRepeatedIndexV1,
}
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub raw_primitives: usize,
    pub triangles: usize,
    pub draw_indices: usize,
    pub array_bytes: usize,
    pub work_units: usize,
    pub max_combined_retained_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            raw_primitives: 4_000_000,
            triangles: 2_000_000,
            draw_indices: 6_000_000,
            array_bytes: 64 * 1024 * 1024,
            work_units: 64_000_000,
            max_combined_retained_bytes: 96 * 1024 * 1024,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    AuthoredTriangles,
    AuthoredStrips,
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Triangle {
    pub source_topology_kind: Kind,
    pub source_primitive_ordinal: usize,
    pub strip_ordinal: Option<usize>,
    pub primitive_step: usize,
    pub local_vertices: [u16; 3],
    pub source_vertices: [u16; 3],
}
#[derive(Clone, Copy, Debug, Serialize)]
pub struct Usage {
    pub raw_primitives: usize,
    pub omitted_connectors: usize,
    pub generated_triangles: usize,
    pub draw_indices: usize,
    pub streams_retained_bytes: usize,
    pub retained_bytes: usize,
    pub combined_retained_bytes: usize,
    pub work_units: usize,
    pub source_decodes: usize,
    pub source_sha256_traversals: usize,
}
#[derive(Debug, Serialize)]
pub struct Packet {
    contract: &'static str,
    policy: Policy,
    identity: Identity,
    source_faces_present: u8,
    declared_source_triangles: u16,
    triangles: Vec<Triangle>,
    usage: Usage,
    retail_behavior_verified: bool,
}
impl Packet {
    pub fn identity(&self) -> &Identity {
        &self.identity
    }
    pub fn triangles(&self) -> &[Triangle] {
        &self.triangles
    }
    pub fn usage(&self) -> Usage {
        self.usage
    }
}
fn string(value: &str, budget: &mut Budget<'_>) -> Result<String> {
    budget.reserve::<u8>(value.len())?;
    budget.charge(value.len())?;
    Ok(value.to_owned())
}
fn span(value: &SourceSpan, budget: &mut Budget<'_>) -> Result<SourceSpan> {
    Ok(SourceSpan {
        block: value.block,
        offset: value.offset,
        bytes: value.bytes,
        sha256: string(&value.sha256, budget)?,
    })
}
fn identity(value: &Identity, budget: &mut Budget<'_>) -> Result<Identity> {
    Ok(Identity {
        source_sha256: string(&value.source_sha256, budget)?,
        geometry: span(&value.geometry, budget)?,
        geometry_data: span(&value.geometry_data, budget)?,
        instance: span(&value.instance, budget)?,
        partition: span(&value.partition, budget)?,
        partition_ordinal: value.partition_ordinal,
        source_vertex_count: value.source_vertex_count,
    })
}
// Source-local repeated indices alone identify connectors. Source-map duplicates
// and geometric area never change this policy. Parity advances for every window.
fn visit(
    topology: &Topology,
    budget: &mut Budget<'_>,
    mut emit: impl FnMut(Triangle, bool, &mut Budget<'_>) -> Result<()>,
) -> Result<()> {
    match topology {
        Topology::Triangles { triangles } => {
            for (ordinal, row) in triangles.iter().enumerate() {
                budget.charge(4)?;
                emit(
                    Triangle {
                        source_topology_kind: Kind::AuthoredTriangles,
                        source_primitive_ordinal: ordinal,
                        strip_ordinal: None,
                        primitive_step: ordinal,
                        local_vertices: row.local_vertices,
                        source_vertices: row.source_vertices,
                    },
                    false,
                    budget,
                )?;
            }
        }
        Topology::Strips { lengths, strips } => {
            let mut ordinal = 0usize;
            for (strip_ordinal, strip) in strips.iter().enumerate() {
                budget.charge(1)?;
                if strip.local_vertices.len() != strip.source_vertices.len()
                    || lengths.get(strip_ordinal).copied().map(usize::from)
                        != Some(strip.local_vertices.len())
                {
                    return Err(budget.fail("sealed strip extent differs"));
                }
                for step in 0..strip.local_vertices.len().saturating_sub(2) {
                    budget.charge(4)?;
                    let mut local = [
                        strip.local_vertices[step],
                        strip.local_vertices[step + 1],
                        strip.local_vertices[step + 2],
                    ];
                    let mut source = [
                        strip.source_vertices[step],
                        strip.source_vertices[step + 1],
                        strip.source_vertices[step + 2],
                    ];
                    let omit = local[0] == local[1] || local[1] == local[2] || local[0] == local[2];
                    if step % 2 == 1 {
                        local.swap(0, 1);
                        source.swap(0, 1);
                    }
                    emit(
                        Triangle {
                            source_topology_kind: Kind::AuthoredStrips,
                            source_primitive_ordinal: ordinal,
                            strip_ordinal: Some(strip_ordinal),
                            primitive_step: step,
                            local_vertices: local,
                            source_vertices: source,
                        },
                        omit,
                        budget,
                    )?;
                    ordinal = ordinal
                        .checked_add(1)
                        .ok_or_else(|| budget.fail("primitive ordinal overflow"))?;
                }
            }
        }
    }
    Ok(())
}
impl Streams {
    pub fn triangle_packet(&self, policy: Policy, limits: Limits) -> Result<Packet> {
        let available = limits
            .max_combined_retained_bytes
            .checked_sub(self.usage.retained_bytes)
            .ok_or_else(|| {
                crate::Error::Unsupported(
                    "partition triangle packet combined storage budget exceeded".into(),
                )
            })?;
        let admitted = limits.array_bytes.min(available);
        let mut budget = Budget {
            source: &self.identity.source_sha256,
            storage: admitted,
            work: limits.work_units,
        };
        budget.reserve::<Packet>(1)?;
        let mut raw = 0usize;
        let mut count = 0usize;
        let mut omitted = 0usize;
        visit(&self.topology, &mut budget, |_, skip, budget| {
            raw = raw
                .checked_add(1)
                .filter(|n| *n <= limits.raw_primitives)
                .ok_or_else(|| budget.fail("raw primitive limit exceeded"))?;
            if skip {
                omitted += 1;
            } else {
                count = count
                    .checked_add(1)
                    .filter(|n| *n <= limits.triangles)
                    .ok_or_else(|| budget.fail("triangle limit exceeded"))?;
            }
            Ok(())
        })?;
        let indices = count
            .checked_mul(3)
            .filter(|n| *n <= limits.draw_indices)
            .ok_or_else(|| budget.fail("triangle index limit exceeded"))?;
        budget.reserve::<Triangle>(count)?;
        let identity = identity(&self.identity, &mut budget)?;
        let mut triangles = Vec::with_capacity(count);
        visit(&self.topology, &mut budget, |row, skip, budget| {
            if !skip {
                budget.charge(8)?;
                triangles.push(row);
            }
            Ok(())
        })?;
        let retained = admitted - budget.storage;
        Ok(Packet {
            contract: "engineering-partition-triangle-packet-v1",
            policy,
            identity,
            source_faces_present: self.presence.faces,
            declared_source_triangles: self.declared_triangles,
            triangles,
            usage: Usage {
                raw_primitives: raw,
                omitted_connectors: omitted,
                generated_triangles: count,
                draw_indices: indices,
                streams_retained_bytes: self.usage.retained_bytes,
                retained_bytes: retained,
                combined_retained_bytes: self.usage.retained_bytes + retained,
                work_units: limits.work_units - budget.work,
                source_decodes: 0,
                source_sha256_traversals: 0,
            },
            retail_behavior_verified: false,
        })
    }
}
