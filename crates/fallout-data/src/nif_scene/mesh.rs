use super::cursor::Reader;
use crate::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Topology {
    Triangles {
        declared_points: u32,
        present: bool,
        indices: Vec<[u16; 3]>,
        match_groups: Vec<Vec<u16>>,
    },
    Strips {
        lengths: Vec<u16>,
        present: bool,
        indices: Vec<Vec<u16>>,
    },
}

#[derive(Debug, Serialize)]
pub struct MeshData {
    pub block: u32,
    pub group_id: i32,
    pub vertex_count: u16,
    pub keep_flags: u8,
    pub compress_flags: u8,
    pub has_vertices: bool,
    pub vertices: Vec<[f32; 3]>,
    pub data_flags: u16,
    pub has_normals: bool,
    pub normals: Vec<[f32; 3]>,
    pub tangents: Vec<[f32; 3]>,
    pub bitangents: Vec<[f32; 3]>,
    pub bound: [f32; 4],
    pub has_colors: bool,
    pub colors: Vec<[f32; 4]>,
    pub uv_sets: Vec<Vec<[f32; 2]>>,
    pub consistency_flags: u16,
    pub additional_data: Option<u32>,
    pub declared_triangles: u16,
    pub topology: Topology,
    /// Strip winding alternates even when a degenerate connector is skipped.
    pub triangles: Vec<[u16; 3]>,
    pub source_triangle_count_matches: bool,
    pub strip_degenerate_triangles: usize,
}

pub(super) fn read(r: &mut Reader<'_>, block: u32, strips: bool) -> Result<MeshData> {
    let group_id = r.u32()? as i32;
    let vertex_count = r.u16()?;
    let count = usize::from(vertex_count);
    let keep_flags = r.u8()?;
    let compress_flags = r.u8()?;
    let has_vertices = r.boolean()?;
    let vertices = r.vectors(if has_vertices { count } else { 0 })?;
    let data_flags = r.u16()?;
    let has_normals = r.boolean()?;
    let normals = r.vectors(if has_normals { count } else { 0 })?;
    let tangent_count = if has_normals && data_flags & 0x1000 != 0 {
        count
    } else {
        0
    };
    let tangents = r.vectors(tangent_count)?;
    let bitangents = r.vectors(tangent_count)?;
    let bound = r.vector()?;
    if bound[3] < 0. {
        return Err(r.fail("negative mesh bounding radius"));
    }
    let has_colors = r.boolean()?;
    let colors = r.vectors(if has_colors { count } else { 0 })?;
    let mut uv_sets = Vec::new();
    if data_flags & 1 != 0 {
        uv_sets.push(r.vectors(count)?);
    }
    let consistency_flags = r.u16()?;
    let additional_data = r.reference()?;
    let declared_triangles = r.u16()?;
    let (topology, triangles) = if strips {
        let strip_count = r.u16()? as usize;
        r.budget(strip_count, 2)?;
        r.reserve::<u16>(strip_count)?;
        let lengths = (0..strip_count)
            .map(|_| r.u16())
            .collect::<Result<Vec<_>>>()?;
        let present = r.boolean()?;
        let mut indices = Vec::new();
        let mut triangles = Vec::new();
        if present {
            // Check the sum before allocating any of the strips.
            r.budget(lengths.iter().map(|n| usize::from(*n)).sum(), 2)?;
            let slots = lengths
                .iter()
                .map(|n| usize::from(*n).saturating_sub(2))
                .sum();
            r.reserve::<[u16; 3]>(slots)?;
            r.reserve::<Vec<u16>>(strip_count)?;
            triangles.reserve_exact(slots);
            indices.reserve_exact(strip_count);
            for &length in &lengths {
                let strip = r.indices(length as usize, vertex_count)?;
                for (step, face) in strip.windows(3).enumerate() {
                    if face[0] == face[1] || face[0] == face[2] || face[1] == face[2] {
                        continue;
                    }
                    triangles.push(if step % 2 == 0 {
                        [face[0], face[1], face[2]]
                    } else {
                        [face[0], face[2], face[1]]
                    });
                }
                indices.push(strip);
            }
        }
        (
            Topology::Strips {
                lengths,
                present,
                indices,
            },
            triangles,
        )
    } else {
        let declared_points = r.u32()?;
        if declared_points != u32::from(declared_triangles) * 3 {
            return Err(r.fail("triangle point count disagrees with triangle count"));
        }
        let present = r.boolean()?;
        let indices = r.indices(
            if present { declared_points as usize } else { 0 },
            vertex_count,
        )?;
        r.reserve::<[u16; 3]>(indices.len() / 3 * 2)?;
        let triangles = indices.as_chunks::<3>().0.to_vec();
        let match_count = r.u16()? as usize;
        r.budget(match_count, 2)?;
        r.reserve::<Vec<u16>>(match_count)?;
        let mut match_groups = Vec::with_capacity(match_count);
        for _ in 0..match_count {
            let count = r.u16()? as usize;
            match_groups.push(r.indices(count, vertex_count)?);
        }
        (
            Topology::Triangles {
                declared_points,
                present,
                indices: triangles.clone(),
                match_groups,
            },
            triangles,
        )
    };
    // NV stores strip primitive slots, including repeated-index connectors, in
    // Num Triangles. Compare that count before removing connectors for drawing.
    let source_count = match &topology {
        Topology::Strips {
            lengths, present, ..
        } if *present => lengths
            .iter()
            .map(|n| usize::from(*n).saturating_sub(2))
            .sum(),
        _ => triangles.len(),
    };
    let source_triangle_count_matches = source_count == usize::from(declared_triangles);
    let strip_degenerate_triangles = if strips {
        source_count - triangles.len()
    } else {
        0
    };
    Ok(MeshData {
        block,
        group_id,
        vertex_count,
        keep_flags,
        compress_flags,
        has_vertices,
        vertices,
        data_flags,
        has_normals,
        normals,
        tangents,
        bitangents,
        bound,
        has_colors,
        colors,
        uv_sets,
        consistency_flags,
        additional_data,
        declared_triangles,
        topology,
        triangles,
        source_triangle_count_matches,
        strip_degenerate_triangles,
    })
}
