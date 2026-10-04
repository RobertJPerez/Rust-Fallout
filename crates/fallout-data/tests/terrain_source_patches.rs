//! Independent physical plugins and archive bytes for aggregate CPU terrain outputs.
use fallout_data::{
    archive::NvArchive,
    identity::{FormKey, ProfileId},
    plugin,
    store::RecordStore,
    terrain::{
        Fields,
        patches::{Limits, TerrainPatchBundle},
    },
    vfs::MountIndex,
    world::cells::{CellGridRequest, CellGridSources},
};
use std::{fs, io::Write};
fn sub(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(data.len() as u16).to_le_bytes(), data].concat()
}
fn record(tag: &[u8; 4], id: u32, flags: u32, data: &[u8]) -> Vec<u8> {
    [
        tag.as_slice(),
        &(data.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 4],
        &15_u16.to_le_bytes(),
        &[0; 2],
        data,
    ]
    .concat()
}
fn header(masters: &[&[u8]]) -> Vec<u8> {
    let mut data = sub(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        data.extend(sub(b"MAST", &[*master, &[0]].concat()));
        data.extend(sub(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &data)
}
fn group(label: u32, kind: i32, data: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(data.len() as u32 + 24).to_le_bytes(),
        &label.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        data,
    ]
    .concat()
}
fn layer(tag: &[u8; 4], quadrant: u8, index: i16) -> Vec<u8> {
    sub(
        tag,
        &[
            0x400_u32.to_le_bytes().as_slice(),
            &[quadrant, 77],
            &index.to_le_bytes(),
        ]
        .concat(),
    )
}
fn land(offset: u32, bad: &str) -> Vec<u8> {
    let mut data = sub(b"DATA", &[7, 0, 0, 0]);
    let mut normals = [255_u8, 0, 0].repeat(1089);
    normals[3..6].copy_from_slice(&[0, 0, 127]);
    if bad == "zero-normal" {
        normals[..3].fill(0);
    }
    if bad == "short-normal" {
        normals.pop();
    }
    data.extend(sub(b"VNML", &normals));
    let mut heights = offset.to_le_bytes().to_vec();
    let mut deltas = vec![0_u8; 1089];
    deltas[..3].copy_from_slice(&[2, 3, 255]);
    deltas[33..35].copy_from_slice(&[252, 5]);
    heights.extend(deltas);
    heights.extend([7, 11, 13]);
    if bad != "missing-height" {
        data.extend(sub(b"VHGT", &heights));
    }
    let colors: Vec<u8> = (0..1089)
        .flat_map(|i| {
            [
                (i % 256) as u8,
                ((i * 3) % 256) as u8,
                (255 - i % 256) as u8,
            ]
        })
        .collect();
    data.extend(sub(b"VCLR", &colors));
    for quadrant in 0..4 {
        data.extend(layer(b"BTXT", quadrant, -1));
    }
    data.extend(layer(b"ATXT", 0, if bad == "layer-order" { 1 } else { 0 }));
    let alpha: Vec<u8> = [
        (0_u16, 0x3f000000_u32),
        (1, 0x3fa00000),
        (2, 0xbe800000),
        (288, 0x3e800000),
    ]
    .into_iter()
    .flat_map(|(at, bits)| [at.to_le_bytes().as_slice(), &[13, 29], &bits.to_le_bytes()].concat())
    .collect();
    data.extend(sub(b"VTXT", &alpha));
    data.extend(layer(b"ATXT", 0, 1));
    data.extend(sub(
        b"VTXT",
        &[
            0_u16.to_le_bytes().as_slice(),
            &[17, 19],
            &0x3f400000_u32.to_le_bytes(),
        ]
        .concat(),
    ));
    data
}
fn source(bad: &str) -> Vec<u8> {
    let mut data = [header(&[]), record(b"WRLD", 0x100, 0, &sub(b"DATA", &[0]))].concat();
    let mut cells = Vec::new();
    for index in 0..2_u32 {
        let hidden = if bad == "hide" && index == 1 {
            16
        } else {
            1 << index
        };
        let flags = 0xaabbcc00_u32 | hidden;
        let x = if bad == "noncardinal" && index == 1 {
            2_i32
        } else {
            index as i32
        };
        let grid = [x.to_le_bytes(), 0_i32.to_le_bytes(), flags.to_le_bytes()].concat();
        let cell = record(
            b"CELL",
            0x200 + index,
            0,
            &[sub(b"DATA", &[0]), sub(b"XCLC", &grid)].concat(),
        );
        let payload = land(
            if index == 0 { 0x3fa00000 } else { 0x40100000 },
            if index == 1 { bad } else { "" },
        );
        let mut lands = if bad == "missing" && index == 1 {
            Vec::new()
        } else {
            record(
                b"LAND",
                0x300 + index,
                if bad == "deleted" && index == 1 {
                    plugin::DELETED
                } else {
                    0
                },
                &payload,
            )
        };
        if bad == "multiple" && index == 1 {
            lands.extend(record(b"LAND", 0x399, 0, &payload));
        }
        if bad == "tainted" && index == 1 {
            let mut encoder =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&payload).unwrap();
            let mut encoded = encoder.finish().unwrap();
            *encoded.last_mut().unwrap() ^= 1;
            let stored = [(payload.len() as u32).to_le_bytes().as_slice(), &encoded].concat();
            lands = record(b"LAND", 0x301, plugin::COMPRESSED, &stored);
        }
        cells.extend(
            [
                cell,
                group(0x200 + index, 6, &group(0x200 + index, 9, &lands)),
            ]
            .concat(),
        );
    }
    data.extend(group(0x100, 1, &cells));
    data.extend(record(
        b"LTEX",
        0x400,
        0,
        &sub(b"TNAM", &0x500_u32.to_le_bytes()),
    ));
    data.extend(record(b"TXST", 0x500, 0, &sub(b"TX00", b"land/a.dds\0")));
    data
}
fn archive() -> Vec<u8> {
    let folder = b"textures\\land\0";
    let name = b"a.dds\0";
    let entry = 53 + folder.len();
    let payload_at = entry + 16 + name.len();
    let mut bytes = vec![0; payload_at];
    bytes[..4].copy_from_slice(b"BSA\0");
    for (at, value) in [
        (4, 104),
        (8, 36),
        (12, 3),
        (16, 1),
        (20, 1),
        (24, folder.len() as u32),
        (28, name.len() as u32),
        (44, 1),
        (48, 52),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[52] = folder.len() as u8;
    bytes[53..entry].copy_from_slice(folder);
    bytes[entry + 8..entry + 12].copy_from_slice(&3_u32.to_le_bytes());
    bytes[entry + 12..entry + 16].copy_from_slice(&(payload_at as u32).to_le_bytes());
    bytes[entry + 16..payload_at].copy_from_slice(name);
    bytes.extend([31, 47, 63]);
    bytes
}
fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "base.esm".into(),
        local_id: id,
    }
}
struct Fixture {
    root: tempfile::TempDir,
    names: Vec<String>,
}
impl Fixture {
    fn new(bad: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("Data")).unwrap();
        fs::write(root.path().join("Data/Base.esm"), source(bad)).unwrap();
        fs::write(root.path().join("Data/authored.bsa"), archive()).unwrap();
        Self {
            root,
            names: vec!["Base.esm".into()],
        }
    }
    fn try_store(&self, forensic: bool) -> fallout_data::Result<RecordStore> {
        let open = if forensic {
            RecordStore::open_nv
        } else {
            RecordStore::open_nv_headers
        };
        open(
            &self.root.path().join("Data"),
            &self.names,
            plugin::Limits {
                inspect_checksum_mismatches: forensic,
                ..Default::default()
            },
        )
    }
    fn store(&self) -> RecordStore {
        self.try_store(false).unwrap()
    }
    fn directory(&self, store: &mut RecordStore) -> CellGridSources {
        CellGridSources::load(store, &key(0x100), Default::default()).unwrap()
    }
    fn mounts(&self) -> MountIndex {
        let mut mounts = MountIndex::default();
        NvArchive::open(&self.root.path().join("Data/authored.bsa"))
            .unwrap()
            .census(&mut mounts)
            .unwrap();
        mounts
    }
    fn requests(&self, directory: &CellGridSources) -> Vec<CellGridRequest> {
        vec![
            directory.request([0, 0]).unwrap(),
            directory.request([1, 0]).unwrap(),
        ]
    }
    fn load(
        &self,
        store: &mut RecordStore,
        directory: &CellGridSources,
        limits: Limits,
    ) -> fallout_data::Result<TerrainPatchBundle> {
        TerrainPatchBundle::load(
            directory,
            store,
            &self.requests(directory),
            &self.mounts(),
            &[[0, 1]],
            limits,
        )
    }
}
#[test]
fn independent_pair_keeps_raw_sources_nonuniform_cpu_geometry_hide_and_weights() {
    let f = Fixture::new("");
    let mut store = f.store();
    let directory = f.directory(&mut store);
    let bundle = f.load(&mut store, &directory, Default::default()).unwrap();
    let r = bundle.receipt();
    assert_eq!(r.world, key(0x100));
    assert_eq!(
        (
            r.usage.patches,
            r.usage.lands,
            r.usage.vertices,
            r.usage.indices,
            r.usage.layers,
            r.usage.weights
        ),
        (2, 2, 2178, 9216, 12, 3468)
    );
    assert_eq!(r.usage.source_records, 10);
    assert_eq!(r.usage.source_read_bytes, 15706);
    assert_eq!(r.usage.source_field_sites, 34);
    assert_eq!(r.usage.mapped_source_bytes, 2 * archive().len() as u64);
    assert_eq!(r.usage.seam_mismatch_slots, 33);
    let first = &r.patches[0];
    assert_eq!(first.source().root, key(0x200));
    assert_eq!(first.source().terrain.cell.header.offset, 97);
    assert_eq!(first.source().terrain.landscapes[0].header.offset, 194);
    assert_eq!(
        first.source().terrain.landscapes[0].header.stored_size,
        7794
    );
    assert_eq!(first.surface.coordinates, [0, 0]);
    assert_eq!(first.raw_quadrant_flags, Some(0xaabbcc01));
    assert_eq!(first.mesh.hidden_quadrants, 1);
    assert_eq!(&first.mesh.indices[..6], &[16, 17, 50, 16, 50, 49]);
    assert_eq!(first.mesh.local_positions[0], [0., 0., 26.]);
    assert_eq!(first.mesh.local_positions[1], [128., 0., 50.]);
    assert_eq!(first.mesh.local_positions[2], [256., 0., 42.]);
    assert_eq!(first.mesh.local_positions[33], [0., 128., -6.]);
    assert_eq!(first.mesh.local_positions[34], [128., 128., 34.]);
    assert_eq!(
        first.mesh.normal_bits.as_ref().unwrap()[0],
        [0xbf800000, 0, 0]
    );
    assert_eq!(
        first.mesh.normal_bits.as_ref().unwrap()[1],
        [0, 0, 0x3f800000]
    );
    assert_eq!(first.mesh.colors.as_ref().unwrap()[1088], [64, 192, 191]);
    let Some(Fields::Land(land)) = &first.source().terrain.landscapes[0].fields else {
        panic!();
    };
    let height = land.heights.as_ref().unwrap();
    assert_eq!(height.decoded_offset, 3283);
    assert_eq!(height.value.offset_bits, 0x3fa00000);
    assert_eq!(height.value.unused, [7, 11, 13]);
    assert_eq!(&height.value.deltas[..3], &[2, 3, -1]);
    assert_eq!(land.layers[4].decoded_offset, 7714);
    assert_eq!(land.layers[4].unused, 77);
    let alpha = land.layers[4].alpha.as_ref().unwrap();
    assert_eq!(alpha.decoded_offset, 7728);
    assert_eq!(alpha.value[0].unused, [13, 29]);
    assert_eq!(alpha.value[0].opacity_bits, 0x3f000000);
    let q = &first.blends.quadrants[0];
    assert_eq!(q.overlays[0].weights[0], 127);
    assert_eq!(q.overlays[1].weights[0], 191);
    assert_eq!(q.base.as_ref().unwrap().weights[0], 0);
    assert_eq!(q.base.as_ref().unwrap().weights[2], 255);
    assert_eq!(q.base.as_ref().unwrap().weights[288], 192);
    assert_eq!(first.blends.clamped_samples, 2);
    assert_eq!(first.blends.overfull_vertices, 1);
    assert!(first.blends.missing_base_quadrants.is_empty());
    let second = &r.patches[1];
    assert_eq!(second.surface.coordinates, [1, 0]);
    assert_eq!(second.raw_quadrant_flags, Some(0xaabbcc02));
    assert_eq!(second.mesh.local_positions[0], [0., 0., 34.]);
    assert_eq!(&second.mesh.indices[..6], &[0, 1, 34, 0, 34, 33]);
    for patch in &r.patches {
        assert!(!patch.source_materials_prepared && !patch.runtime_ready);
        assert_eq!(
            patch.mesh.model,
            "esm4-source-grid-checkerboard-positive-z-v1"
        );
        assert_eq!(patch.blends.model, "esm4-local-u8-residual-base-v1");
    }
    assert!(!r.runtime_ready);
}
#[test]
fn explicit_seams_keep_every_literal_mismatch_and_source_surfaces_unchanged() {
    let f = Fixture::new("");
    let mut store = f.store();
    let directory = f.directory(&mut store);
    let requests = f.requests(&directory);
    let base = TerrainPatchBundle::load(
        &directory,
        &mut store,
        &requests,
        &f.mounts(),
        &[],
        Default::default(),
    )
    .unwrap();
    let seam = f.load(&mut store, &directory, Default::default()).unwrap();
    for i in 0..2 {
        assert_eq!(base.patches()[i].identity, seam.patches()[i].identity);
    }
    let c = &seam.receipt().seams[0].comparison;
    assert_eq!(c.direction, "east");
    assert_eq!(c.samples_compared, 33);
    assert!(!c.exact_bits_equal);
    assert_eq!(c.maximum_absolute_difference, 32.);
    assert_eq!(c.mismatches.len(), 33);
    for (i, m) in c.mismatches.iter().enumerate() {
        assert_eq!(m.sample, i);
        assert_eq!(
            m.first_bits,
            if i == 0 {
                0x42280000
            } else if i == 1 {
                0x42080000
            } else {
                0xc0c00000
            }
        );
        assert_eq!(m.second_bits, if i == 0 { 0x42080000 } else { 0x40000000 });
    }
    let reverse = TerrainPatchBundle::load(
        &directory,
        &mut store,
        &requests,
        &f.mounts(),
        &[[1, 0]],
        Default::default(),
    )
    .unwrap();
    assert_eq!(reverse.receipt().seams[0].comparison.direction, "west");
    assert_eq!(
        reverse.receipt().seams[0].comparison.mismatches[0].first_bits,
        0x42080000
    );
    for pairs in [&[[0, 0]][..], &[[0, 2]][..]] {
        assert!(
            TerrainPatchBundle::load(
                &directory,
                &mut store,
                &requests,
                &f.mounts(),
                pairs,
                Default::default()
            )
            .is_err()
        );
    }
    assert_ne!(base.identity(), seam.identity());
    let f = Fixture::new("noncardinal");
    let mut store = f.store();
    let directory = f.directory(&mut store);
    let requests = [
        directory.request([0, 0]).unwrap(),
        directory.request([2, 0]).unwrap(),
    ];
    assert!(
        TerrainPatchBundle::load(
            &directory,
            &mut store,
            &requests,
            &f.mounts(),
            &[],
            Default::default()
        )
        .is_ok()
    );
    assert!(
        TerrainPatchBundle::load(
            &directory,
            &mut store,
            &requests,
            &f.mounts(),
            &[[0, 1]],
            Default::default()
        )
        .is_err()
    );
}
#[test]
fn all_fifteen_exact_allowances_and_one_under_refuse_without_partial_cpu_bundle() {
    let f = Fixture::new("");
    let mut store = f.store();
    let directory = f.directory(&mut store);
    let source = f.load(&mut store, &directory, Default::default()).unwrap();
    let u = &source.receipt().usage;
    let exact = Limits {
        patches: u.patches,
        lands: u.lands,
        source_records: u.source_records,
        source_read_bytes: u.source_read_bytes,
        source_field_sites: u.source_field_sites,
        source_metadata_bytes: u.source_metadata_bytes,
        mapped_source_bytes: u.mapped_source_bytes,
        vertices: u.vertices,
        indices: u.indices,
        layers: u.layers,
        weights: u.weights,
        output_bytes: u.output_bytes,
        metadata_bytes: u.metadata_bytes,
        seams: u.seams,
        seam_mismatch_slots: u.seam_mismatch_slots,
        ..Default::default()
    };
    let identity = source.identity().to_owned();
    assert_eq!(
        f.load(&mut store, &directory, exact).unwrap().identity(),
        identity
    );
    let mut limits = Vec::new();
    macro_rules! under {
        ($field:ident) => {
            limits.push(Limits {
                $field: exact.$field - 1,
                ..exact
            });
        };
    }
    under!(patches);
    under!(lands);
    under!(source_records);
    under!(source_read_bytes);
    under!(source_field_sites);
    under!(source_metadata_bytes);
    under!(mapped_source_bytes);
    under!(vertices);
    under!(indices);
    under!(layers);
    under!(weights);
    under!(output_bytes);
    under!(metadata_bytes);
    under!(seams);
    under!(seam_mismatch_slots);
    for limit in limits {
        assert!(f.load(&mut store, &directory, limit).is_err());
    }
    assert_eq!(
        f.load(&mut store, &directory, exact).unwrap().identity(),
        identity
    );
    assert!(
        f.load(
            &mut store,
            &directory,
            Limits {
                patches: 9,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert!(
        f.load(
            &mut store,
            &directory,
            Limits {
                source: fallout_data::terrain::preparation::Limits {
                    layers: 4097,
                    ..Default::default()
                },
                ..Default::default()
            }
        )
        .is_err()
    );
}
#[test]
fn missing_multiple_deleted_tainted_invalid_normals_hide_and_layers_never_publish_cpu_outputs() {
    for bad in [
        "missing",
        "multiple",
        "deleted",
        "missing-height",
        "short-normal",
        "zero-normal",
        "hide",
        "layer-order",
    ] {
        let f = Fixture::new(bad);
        assert!(
            f.try_store(false)
                .and_then(|mut store| {
                    let directory = f.directory(&mut store);
                    f.load(&mut store, &directory, Default::default())
                })
                .is_err(),
            "{bad}"
        );
    }
    let f = Fixture::new("tainted");
    assert!(
        f.try_store(false)
            .and_then(|mut store| {
                let directory = f.directory(&mut store);
                f.load(&mut store, &directory, Default::default())
            })
            .is_err()
    );
    let mut store = f.try_store(true).unwrap();
    let directory = f.directory(&mut store);
    assert!(f.load(&mut store, &directory, Default::default()).is_err());
}
#[test]
fn caller_order_duplicate_empty_cross_world_and_noncardinal_requests_are_checked() {
    let f = Fixture::new("");
    let mut store = f.store();
    let directory = f.directory(&mut store);
    let requests = f.requests(&directory);
    let reverse = [requests[1].clone(), requests[0].clone()];
    let b = TerrainPatchBundle::load(
        &directory,
        &mut store,
        &reverse,
        &f.mounts(),
        &[],
        Default::default(),
    )
    .unwrap();
    assert_eq!(b.patches()[0].source().root, key(0x201));
    assert_eq!(b.patches()[1].source().root, key(0x200));
    let duplicate = [requests[0].clone(), requests[0].clone()];
    assert!(
        TerrainPatchBundle::load(
            &directory,
            &mut store,
            &duplicate,
            &f.mounts(),
            &[],
            Default::default()
        )
        .is_err()
    );
    assert!(
        TerrainPatchBundle::load(
            &directory,
            &mut store,
            &[],
            &f.mounts(),
            &[],
            Default::default()
        )
        .is_err()
    );
    drop(store);
    let mut data = source("");
    data.extend(record(b"WRLD", 0x101, 0, &sub(b"DATA", &[0])));
    data.extend(group(
        0x101,
        1,
        &record(
            b"CELL",
            0x299,
            0,
            &[
                sub(b"DATA", &[0]),
                sub(
                    b"XCLC",
                    &[2_i32.to_le_bytes(), 0_i32.to_le_bytes()].concat(),
                ),
            ]
            .concat(),
        ),
    ));
    fs::write(f.root.path().join("Data/Base.esm"), data).unwrap();
    let mut store = f.store();
    let changed = f.directory(&mut store);
    let other = CellGridSources::load(&mut store, &key(0x101), Default::default()).unwrap();
    let cross = [
        changed.request([0, 0]).unwrap(),
        other.request([2, 0]).unwrap(),
    ];
    assert!(
        TerrainPatchBundle::load(
            &changed,
            &mut store,
            &cross,
            &f.mounts(),
            &[],
            Default::default()
        )
        .is_err()
    );
    assert!(
        TerrainPatchBundle::load(
            &changed,
            &mut store,
            &requests,
            &f.mounts(),
            &[],
            Default::default()
        )
        .is_err()
    );
}
#[test]
fn reuse_refuses_changed_source_names_order_count_and_unselected_bytes() {
    let mut f = Fixture::new("");
    fs::write(f.root.path().join("Data/Other.esm"), header(&[])).unwrap();
    fs::write(f.root.path().join("Data/Third.esm"), header(&[])).unwrap();
    f.names.extend(["Other.esm".into(), "Third.esm".into()]);
    let mut store = f.store();
    let directory = f.directory(&mut store);
    let bundle = f.load(&mut store, &directory, Default::default()).unwrap();
    drop(store);
    f.names.swap(1, 2);
    assert!(bundle.validate_sources(&mut f.store()).is_err());
    f.names.swap(1, 2);
    f.names.pop();
    assert!(bundle.validate_sources(&mut f.store()).is_err());
    f.names.push("Third.esm".into());
    fs::copy(
        f.root.path().join("Data/Other.esm"),
        f.root.path().join("Data/Renamed.esm"),
    )
    .unwrap();
    f.names[1] = "Renamed.esm".into();
    assert!(bundle.validate_sources(&mut f.store()).is_err());
    f.names[1] = "Other.esm".into();
    fs::write(
        f.root.path().join("Data/Third.esm"),
        [header(&[]), record(b"STAT", 0x900, 0, &[1, 2, 3])].concat(),
    )
    .unwrap();
    assert!(bundle.validate_sources(&mut f.store()).is_err());
}
#[cfg(windows)]
#[test]
fn privately_retained_archive_plans_keep_source_pins_until_last_bundle_drop() {
    let f = Fixture::new("");
    let path = f.root.path().join("Data/authored.bsa");
    let mut store = f.store();
    let directory = f.directory(&mut store);
    let bundle = f.load(&mut store, &directory, Default::default()).unwrap();
    drop(store);
    drop(directory);
    assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
    assert_eq!(bundle.patches()[0].mesh.local_positions[1], [128., 0., 50.]);
    drop(bundle);
    assert!(fs::OpenOptions::new().write(true).open(&path).is_ok());
}
