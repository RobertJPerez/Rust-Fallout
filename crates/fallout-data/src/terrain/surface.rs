//! Derived heights retain their winning record identity and never fill a missing
//! or deleted LAND with world defaults or a parent world's surface.
use super::{
    Fields, TerrainReport,
    heights::{self, EdgeComparison, HeightGrid},
};
use crate::{Error, Result, identity::FormKey};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Surface {
    pub cell: FormKey,
    pub world: FormKey,
    pub coordinates: [i32; 2],
    pub landscapes: Vec<SurfaceLand>,
    pub scope: &'static str,
}

#[derive(Debug, Serialize)]
pub struct SurfaceLand {
    pub key: FormKey,
    pub decoded_sha256: Option<String>,
    pub height_field_offset: Option<usize>,
    pub status: &'static str,
    pub height_grid: Option<HeightGrid>,
}

pub fn reconstruct_cell(report: &TerrainReport) -> Result<Surface> {
    let Some(Fields::Cell(cell)) = &report.cell.fields else {
        return Err(Error::Resolution(
            "surface requires an inspected exterior CELL".into(),
        ));
    };
    let coordinates = cell
        .grid
        .as_ref()
        .ok_or_else(|| Error::Resolution("surface CELL has no XCLC".into()))?
        .value;
    let world = report
        .cell
        .links
        .get("group.world")
        .filter(|link| link.status == "resolved")
        .and_then(|link| link.key.clone())
        .ok_or_else(|| Error::Resolution("surface CELL has no resolved worldspace".into()))?;
    let mut landscapes = Vec::with_capacity(report.landscapes.len());
    for record in &report.landscapes {
        let height = match &record.fields {
            Some(Fields::Land(land)) => land.heights.as_ref(),
            None if record.header.flags & crate::plugin::DELETED != 0 => None,
            _ => {
                return Err(Error::Resolution(
                    "surface contains a non-LAND record".into(),
                ));
            }
        };
        let height_grid = height
            .map(|field| heights::reconstruct(&field.value))
            .transpose()?;
        landscapes.push(SurfaceLand {
            key: record.key.clone(),
            decoded_sha256: record.decoded_sha256.clone(),
            height_field_offset: height.map(|field| field.decoded_offset),
            status: if height.is_some() {
                "reconstructed"
            } else if record.fields.is_none() {
                "deleted"
            } else {
                "missing_vhgt"
            },
            height_grid,
        });
    }
    Ok(Surface {
        cell: report.cell.key.clone(),
        world,
        coordinates,
        landscapes,
        scope: "Pinned ESM4 source-height convention; no retail measurement, inheritance, defaults, mesh, normals, interpolation or physics acceptance",
    })
}

pub fn compare_neighbor(first: &Surface, second: &Surface) -> Result<EdgeComparison> {
    if first.world != second.world {
        return Err(Error::Resolution(
            "height edges belong to different worldspaces".into(),
        ));
    }
    fn single(surface: &Surface) -> Result<&HeightGrid> {
        if surface.landscapes.len() != 1 {
            return Err(Error::Resolution(
                "edge comparison requires exactly one LAND per cell".into(),
            ));
        }
        surface.landscapes[0]
            .height_grid
            .as_ref()
            .ok_or_else(|| Error::Resolution("edge comparison requires a present VHGT".into()))
    }
    heights::compare_edges(
        first.coordinates,
        single(first)?,
        second.coordinates,
        single(second)?,
    )
}
