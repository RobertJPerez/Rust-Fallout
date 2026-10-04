use super::*;
use crate::{Error, Result, malformed};
use sha2::{Digest, Sha256};

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    name: &'a str,
    base: u64,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(n)
            .filter(|v| *v <= self.data.len())
            .ok_or_else(|| {
                malformed(
                    self.name,
                    self.base + self.at as u64,
                    "truncated navigation field",
                )
            })?;
        let out = &self.data[self.at..end];
        self.at = end;
        Ok(out)
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("checked field"),
        ))
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("checked field"),
        ))
    }
    fn float(&mut self) -> Result<f32> {
        let v = f32::from_bits(self.u32()?);
        if !v.is_finite() {
            return Err(malformed(
                self.name,
                self.base + self.at as u64 - 4,
                "nonfinite navigation value",
            ));
        }
        Ok(v)
    }
    fn vector(&mut self) -> Result<[f32; 3]> {
        Ok([self.float()?, self.float()?, self.float()?])
    }
    fn finish(&self) -> Result<()> {
        if self.at != self.data.len() {
            return Err(malformed(
                self.name,
                self.base + self.at as u64,
                "surplus navigation field bytes",
            ));
        }
        Ok(())
    }
    fn count(&mut self, count: usize, width: usize, left: &mut usize) -> Result<()> {
        if count > *left {
            return Err(Error::Unsupported("navigation element budget".into()));
        }
        if count
            .checked_mul(width)
            .is_none_or(|bytes| bytes > self.data.len() - self.at)
        {
            return Err(malformed(
                self.name,
                self.base + self.at as u64,
                "navigation count exceeds remaining source field",
            ));
        }
        *left -= count;
        Ok(())
    }
    fn forms(&mut self, left: &mut usize) -> Result<Vec<u32>> {
        let count = self.u32()? as usize;
        self.count(count, 4, left)?;
        (0..count).map(|_| self.u32()).collect()
    }
}
fn fields(
    record: &plugin::Record,
    name: &str,
    kind: [u8; 4],
    limits: Limits,
) -> Result<Vec<RawField>> {
    if record.header.kind != kind
        || record.integrity_issue.is_some()
        || record.header.flags & plugin::DELETED != 0
    {
        return Err(malformed(
            name,
            record.header.offset,
            "navigation needs an untainted, live record of the selected kind",
        ));
    }
    if record.payload.len() > limits.record_bytes {
        return Err(Error::Unsupported("navigation record byte budget".into()));
    }
    let mut fields = Vec::new();
    plugin::visit_subrecords(record, name, |s| {
        if fields.len() >= limits.fields {
            return Err(Error::Unsupported("navigation field budget".into()));
        }
        fields.push(RawField {
            kind: s.kind,
            decoded_offset: s.payload_offset,
            bytes: s.data.to_vec(),
        });
        Ok(())
    })?;
    Ok(fields)
}
fn singleton<'a>(
    fields: &'a [RawField],
    kind: [u8; 4],
    name: &str,
    offset: u64,
) -> Result<Option<&'a RawField>> {
    let mut matching = fields.iter().filter(|f| f.kind == kind);
    let first = matching.next();
    if matching.next().is_some() {
        return Err(malformed(
            name,
            offset,
            format!("duplicate navigation {}", plugin::signature(kind)),
        ));
    }
    Ok(first)
}
fn required<'a>(
    fields: &'a [RawField],
    kind: [u8; 4],
    name: &str,
    offset: u64,
) -> Result<&'a RawField> {
    singleton(fields, kind, name, offset)?.ok_or_else(|| {
        malformed(
            name,
            offset,
            format!("missing navigation {}", plugin::signature(kind)),
        )
    })
}
fn reader<'a>(field: &'a RawField, name: &'a str) -> Reader<'a> {
    Reader {
        data: &field.bytes,
        at: 0,
        name,
        base: field.decoded_offset as u64 + 6,
    }
}

pub fn decode_mesh(record: &plugin::Record, name: &str, limits: Limits) -> Result<NavMesh> {
    let fields = fields(record, name, *b"NAVM", limits)?;
    let mut left = limits.elements;
    let version_field = required(&fields, *b"NVER", name, record.header.offset)?;
    let mut r = reader(version_field, name);
    let version = r.u32()?;
    r.finish()?;
    let data = required(&fields, *b"DATA", name, record.header.offset)?;
    let mut r = reader(data, name);
    let cell = r.u32()?;
    let counts = [
        r.u32()? as usize,
        r.u32()? as usize,
        r.u32()? as usize,
        r.u32()? as usize,
        r.u32()? as usize,
    ];
    r.finish()?;
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    let mut edge_links = Vec::new();
    let mut cover = Vec::new();
    let mut doors = Vec::new();
    for (index, kind, width) in [
        (0, *b"NVVX", 12),
        (1, *b"NVTR", 16),
        (2, *b"NVEX", 10),
        (3, *b"NVCA", 2),
        (4, *b"NVDP", 8),
    ] {
        let field = singleton(&fields, kind, name, record.header.offset)?;
        if field.is_none() && counts[index] != 0 {
            return Err(malformed(
                name,
                record.header.offset,
                format!("missing navigation {}", plugin::signature(kind)),
            ));
        }
        let Some(field) = field else {
            continue;
        };
        let mut r = reader(field, name);
        r.count(counts[index], width, &mut left)?;
        for _ in 0..counts[index] {
            match index {
                0 => vertices.push(r.vector()?),
                1 => triangles.push(Triangle {
                    vertices: [r.u16()?, r.u16()?, r.u16()?],
                    edges: [r.i16()?, r.i16()?, r.i16()?],
                    flags: r.u16()?,
                    cover_flags: r.u16()?,
                }),
                2 => edge_links.push(EdgeLink {
                    link_type: r.u32()?,
                    navmesh_raw: r.u32()?,
                    triangle: r.u16()?,
                }),
                3 => cover.push(r.u16()?),
                4 => doors.push(DoorLink {
                    door_raw: r.u32()?,
                    triangle: r.u16()?,
                    unused: r.take(2)?.try_into().expect("checked field"),
                }),
                _ => unreachable!("fixed field kind"),
            }
        }
        r.finish()?;
    }
    for triangle in &triangles {
        if triangle
            .vertices
            .iter()
            .any(|v| usize::from(*v) >= vertices.len())
        {
            return Err(malformed(
                name,
                record.header.offset,
                "navigation triangle vertex outside NVVX",
            ));
        }
        for edge in 0..3 {
            let link = triangle.edges[edge];
            if link >= 0
                && (if triangle.flags & (1 << edge) != 0 {
                    link as usize >= edge_links.len()
                } else {
                    link as usize >= triangles.len()
                })
            {
                return Err(malformed(
                    name,
                    record.header.offset,
                    "navigation triangle edge outside authored table",
                ));
            }
        }
    }
    if cover.iter().any(|v| usize::from(*v) >= triangles.len())
        || doors
            .iter()
            .any(|v| usize::from(v.triangle) >= triangles.len())
    {
        return Err(malformed(
            name,
            record.header.offset,
            "navigation cover/door triangle outside NVTR",
        ));
    }
    Ok(NavMesh {
        header: record.header.clone(),
        decoded_bytes: record.payload.len(),
        decoded_sha256: format!("{:x}", Sha256::digest(&record.payload)),
        version: SourceField {
            decoded_offset: version_field.decoded_offset,
            value: version,
        },
        cell_raw: SourceField {
            decoded_offset: data.decoded_offset,
            value: cell,
        },
        vertices,
        triangles,
        edge_links,
        cover_triangles: cover,
        door_links: doors,
        fields,
    })
}

pub fn decode_info_map(record: &plugin::Record, name: &str, limits: Limits) -> Result<InfoMap> {
    let fields = fields(record, name, *b"NAVI", limits)?;
    let mut left = limits.elements;
    let version_field = required(&fields, *b"NVER", name, record.header.offset)?;
    let mut r = reader(version_field, name);
    let version = r.u32()?;
    r.finish()?;
    let mut infos = Vec::new();
    let mut connections = Vec::new();
    for field in &fields {
        let mut r = reader(field, name);
        match &field.kind {
            b"NVMI" => {
                if left == 0 {
                    return Err(Error::Unsupported("navigation info budget".into()));
                }
                left -= 1;
                let flags = r.u32()?;
                let navmesh_raw = r.u32()?;
                let location_raw = r.u32()?;
                let grid_y = r.i16()?;
                let grid_x = r.i16()?;
                let approximate_location = r.vector()?;
                let island = if flags & 0x20 != 0 {
                    let bounds = [r.vector()?, r.vector()?];
                    let nv = r.u16()? as usize;
                    let nt = r.u16()? as usize;
                    r.count(nv, 12, &mut left)?;
                    let vertices = (0..nv).map(|_| r.vector()).collect::<Result<Vec<_>>>()?;
                    r.count(nt, 6, &mut left)?;
                    let triangles = (0..nt)
                        .map(|_| Ok([r.u16()?, r.u16()?, r.u16()?]))
                        .collect::<Result<Vec<_>>>()?;
                    if triangles
                        .iter()
                        .flatten()
                        .any(|i| usize::from(*i) >= vertices.len())
                    {
                        return Err(malformed(
                            name,
                            field.decoded_offset as u64,
                            "NAVI island triangle vertex out of range",
                        ));
                    }
                    Some(Island {
                        bounds,
                        vertices,
                        triangles,
                    })
                } else {
                    None
                };
                let preferred_percent = r.float()?;
                r.finish()?;
                infos.push(MeshInfo {
                    decoded_offset: field.decoded_offset,
                    flags,
                    navmesh_raw,
                    location_raw,
                    grid_y,
                    grid_x,
                    approximate_location,
                    island,
                    preferred_percent,
                });
            }
            b"NVCI" => {
                if left == 0 {
                    return Err(Error::Unsupported("navigation connection budget".into()));
                }
                left -= 1;
                let navmesh_raw = r.u32()?;
                let standard = r.forms(&mut left)?;
                let preferred = r.forms(&mut left)?;
                let doors = r.forms(&mut left)?;
                r.finish()?;
                connections.push(Connections {
                    decoded_offset: field.decoded_offset,
                    navmesh_raw,
                    standard,
                    preferred,
                    doors,
                });
            }
            _ => {}
        }
    }
    Ok(InfoMap {
        header: record.header.clone(),
        decoded_bytes: record.payload.len(),
        decoded_sha256: format!("{:x}", Sha256::digest(&record.payload)),
        version: SourceField {
            decoded_offset: version_field.decoded_offset,
            value: version,
        },
        infos,
        connections,
        fields,
    })
}
