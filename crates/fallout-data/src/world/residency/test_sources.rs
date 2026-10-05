//! Independently authored CELL, XTEL, NIF and BSA104 bytes for connected consumers.
use super::*;
use crate::{archive::NvArchive, identity::ProfileId, plugin, store::RecordStore, vfs::MountIndex};
use std::{
    fs,
    io::Write,
    path::Path,
    thread,
    time::{Duration, Instant},
};
pub(super) const POSE: [u32; 6] = [
    0x80000000, 0x41480000, 0xc1c80000, 0x3f800000, 0x40000000, 0x40400000,
];
pub(super) const LIGHT_WORDS: [u32; 10] = [
    0x04030201, 0x1713110d, 0x44332211, 0x80000000, 0x7fc00001, 0xfffffff9, 0x80000000, 0x3e800000,
    0x3f800000, 0x40000000,
];
pub(super) fn scene_fields() -> Vec<u8> {
    [
        field(
            b"XCLL",
            &LIGHT_WORDS
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        ),
        field(b"LTMP", &0x500u32.to_le_bytes()),
        field(b"LNAM", &0x2cu32.to_le_bytes()),
        field(b"XCLW", &0x7fc00001u32.to_le_bytes()),
        field(b"XCWT", &0x501u32.to_le_bytes()),
        field(b"XNAM", b"n.dds\0"),
    ]
    .concat()
}
pub(in crate::world) fn key(id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "base.esm".into(),
        local_id: id,
    }
}
pub(in crate::world) fn field(tag: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
pub(in crate::world) fn record(tag: &[u8; 4], id: u32, flags: u32, bytes: &[u8]) -> Vec<u8> {
    [
        tag.as_slice(),
        &(bytes.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &id.to_le_bytes(),
        &[0; 8],
        bytes,
    ]
    .concat()
}
pub(super) fn group(label: u32, kind: i32, bytes: &[u8]) -> Vec<u8> {
    [
        b"GRUP".as_slice(),
        &(bytes.len() as u32 + 24).to_le_bytes(),
        &label.to_le_bytes(),
        &kind.to_le_bytes(),
        &[0; 8],
        bytes,
    ]
    .concat()
}
pub(in crate::world) fn reference(id: u32, base: u32, flags: u32, extra: &[u8]) -> Vec<u8> {
    let target_data: Vec<u8> = [99.0f32, 88.0, 77.0, -7.0, -8.0, -9.0]
        .into_iter()
        .flat_map(|v| v.to_bits().to_le_bytes())
        .collect();
    record(
        b"REFR",
        id,
        flags,
        &[
            field(b"NAME", &base.to_le_bytes()),
            field(b"DATA", &target_data),
            extra.to_vec(),
        ]
        .concat(),
    )
}
pub(super) fn teleport(target: u32) -> Vec<u8> {
    field(
        b"XTEL",
        &[
            target.to_le_bytes().as_slice(),
            &POSE
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
            &0x81234567u32.to_le_bytes(),
        ]
        .concat(),
    )
}
pub(super) fn members(cell: u32, refs: &[u8]) -> Vec<u8> {
    group(cell, 6, &group(cell, 9, refs))
}
fn nif(texture: &[u8]) -> Vec<u8> {
    let payload = [
        1u32.to_le_bytes().as_slice(),
        &(texture.len() as u32).to_le_bytes(),
        texture,
    ]
    .concat();
    let name = b"BSShaderTextureSet";
    let mut out = b"Gamebryo File Format, Version 20.2.0.7\n".to_vec();
    out.extend(0x14020007u32.to_le_bytes());
    out.push(1);
    for v in [11u32, 1, 34] {
        out.extend(v.to_le_bytes());
    }
    out.extend([0; 3]);
    out.extend(1u16.to_le_bytes());
    out.extend((name.len() as u32).to_le_bytes());
    out.extend(name);
    out.extend(0u16.to_le_bytes());
    out.extend((payload.len() as u32).to_le_bytes());
    out.extend([0; 12]);
    out.extend(payload);
    out.extend([0; 4]);
    out
}
fn archive(path: &Path, folder: &[u8], names: &[&[u8]], payloads: &[Vec<u8>]) {
    let names_bytes: Vec<u8> = names
        .iter()
        .flat_map(|s| s.iter().copied().chain([0]))
        .collect();
    let table = 54 + folder.len();
    let data_offset = table + 16 * names.len() + names_bytes.len();
    let mut out = vec![0; data_offset];
    out[..4].copy_from_slice(b"BSA\0");
    for (at, v) in [
        (4, 104),
        (8, 36),
        (12, 7),
        (16, 1),
        (20, names.len() as u32),
        (24, folder.len() as u32 + 1),
        (28, names_bytes.len() as u32),
        (44, names.len() as u32),
        (48, 52),
    ] {
        out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    out[52] = (folder.len() + 1) as u8;
    out[53..53 + folder.len()].copy_from_slice(folder);
    out[table + 16 * names.len()..].copy_from_slice(&names_bytes);
    for (i, bytes) in payloads.iter().enumerate() {
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(bytes).unwrap();
        let body = [
            (bytes.len() as u32).to_le_bytes().as_slice(),
            &z.finish().unwrap(),
        ]
        .concat();
        let at = table + 16 * i;
        let offset = out.len() as u32;
        out[at..at + 8].copy_from_slice(&(i as u64 + 1).to_le_bytes());
        out[at + 8..at + 12].copy_from_slice(&(body.len() as u32).to_le_bytes());
        out[at + 12..at + 16].copy_from_slice(&offset.to_le_bytes());
        out.extend(body);
    }
    fs::write(path, out).unwrap();
}

fn corrupt_first_archive_member(path: &Path, folder: &[u8]) {
    let mut bytes = fs::read(path).unwrap();
    let table = 54 + folder.len();
    let offset = u32::from_le_bytes(bytes[table + 12..table + 16].try_into().unwrap()) as usize;
    let compressed = offset + 4;
    bytes[compressed..compressed + 2].copy_from_slice(&[0, 0]);
    fs::write(path, bytes).unwrap();
}

pub(in crate::world) struct Fixture {
    pub root: tempfile::TempDir,
    pub cache: tempfile::TempDir,
    pub models: Vec<Vec<u8>>,
    // These source payloads are consumed by the full-tree environment and prefetch tests.
    #[allow(dead_code)]
    pub textures: Vec<Vec<u8>>,
    #[allow(dead_code)]
    pub noise: Vec<u8>,
    pub mounts: MountIndex,
    names: Vec<String>,
}
impl Fixture {
    pub fn new() -> Self {
        Self::with_scene(false, false)
    }
    // The direct residency transport excludes the full-scene consumers of this fixture.
    #[allow(dead_code)]
    pub fn scene() -> Self {
        Self::with_scene(true, false)
    }
    #[allow(dead_code)]
    pub fn selected_scene() -> Self {
        Self::with_scene(true, true)
    }
    fn with_scene(scene: bool, selection: bool) -> Self {
        Self::with_source_error(scene, selection, false)
    }
    pub fn with_corrupt_model_archive() -> Self {
        Self::with_source_error(false, false, true)
    }
    fn with_source_error(scene: bool, selection: bool, corrupt_model_archive: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let data = root.path().join("Data");
        fs::create_dir(&data).unwrap();
        let models: Vec<_> = [b"a.dds".as_slice(), b"b.dds", b"c.dds"]
            .into_iter()
            .map(nif)
            .collect();
        let textures = vec![
            b"persistent texture".to_vec(),
            b"west texture".to_vec(),
            b"east texture".to_vec(),
        ];
        archive(
            &data.join("models.bsa"),
            b"meshes",
            &[b"p.nif", b"w.nif", b"e.nif"],
            &models,
        );
        if corrupt_model_archive {
            corrupt_first_archive_member(&data.join("models.bsa"), b"meshes");
        }
        let noise = b"authored noise source".to_vec();
        let mut texture_names: Vec<&[u8]> = vec![b"a.dds", b"b.dds", b"c.dds"];
        let mut texture_payloads = textures.clone();
        if scene {
            texture_names.push(b"n.dds");
            texture_payloads.push(noise.clone());
        }
        archive(
            &data.join("textures.bsa"),
            b"textures",
            &texture_names,
            &texture_payloads,
        );
        let mut mounts = MountIndex::default();
        for label in ["models", "textures"] {
            NvArchive::open(&data.join(format!("{label}.bsa")))
                .unwrap()
                .census(&mut mounts)
                .unwrap();
        }
        let mut esm = record(
            b"TES4",
            0,
            0,
            &field(
                b"HEDR",
                &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
            ),
        );
        for (i, path) in [b"p.nif\0".as_slice(), b"w.nif\0", b"e.nif\0"]
            .into_iter()
            .enumerate()
        {
            esm.extend(record(b"STAT", 0x400 + i as u32, 0, &field(b"MODL", path)));
        }
        esm.extend(record(b"DOOR", 0x450, 0, &field(b"MODL", b"p.nif\0")));
        if scene {
            esm.extend(record(
                b"LGTM",
                0x500,
                0,
                &field(
                    b"DATA",
                    &LIGHT_WORDS
                        .into_iter()
                        .map(|w| w ^ 0x01010101)
                        .flat_map(u32::to_le_bytes)
                        .collect::<Vec<_>>(),
                ),
            ));
            esm.extend(record(
                b"WATR",
                0x501,
                0,
                &field(b"EDID", b"AuthoredWater\0"),
            ));
        }
        if selection {
            esm.extend(record(
                b"STAT",
                0x499,
                0,
                &field(b"MODL", b"C:\\export\\unsafe.nif\0"),
            ));
        }
        esm.extend(record(b"WRLD", 0x100, 0, &field(b"DATA", &[0])));
        let mut cells = Vec::new();
        for i in 0..3u32 {
            let grid = if i == 0 {
                Vec::new()
            } else {
                field(
                    b"XCLC",
                    &[
                        (i as i32 - 1).to_le_bytes().as_slice(),
                        &0i32.to_le_bytes(),
                        &0xaabbcc00u32.to_le_bytes(),
                    ]
                    .concat(),
                )
            };
            cells.extend(record(
                b"CELL",
                0x200 + i,
                if i == 0 { 0x400 } else { 0 },
                &[
                    field(b"DATA", &[0]),
                    grid,
                    if scene && i == 0 {
                        scene_fields()
                    } else {
                        Vec::new()
                    },
                ]
                .concat(),
            ));
            let mut refs = reference(0x300 + i, 0x400 + i, 0, &[]);
            if i == 0 {
                refs.extend(reference(0x311, 0x450, 0, &[]));
                if selection {
                    refs.extend(reference(0x312, 0x400, 0, &[]));
                    refs.extend(reference(0x313, 0x401, 0, &[]));
                    refs.extend(reference(0x314, 0x499, 0, &[]));
                }
            }
            if i == 1 {
                refs.extend(reference(0x310, 0x450, 0, &teleport(0x311)));
            }
            cells.extend(members(0x200 + i, &refs));
        }
        esm.extend(group(0x100, 1, &cells));
        fs::write(data.join("Base.esm"), esm).unwrap();
        fs::write(root.path().join("order.json"), b"[\"Base.esm\"]").unwrap();
        Self {
            root,
            cache,
            models,
            textures,
            noise,
            mounts,
            names: vec!["Base.esm".into()],
        }
    }
    pub fn store(&self) -> RecordStore {
        RecordStore::open_nv_headers(
            &self.root.path().join("Data"),
            &self.names,
            plugin::Limits::default(),
        )
        .unwrap()
    }
    pub fn plans(&self) -> Vec<CellModelPlan> {
        let mut store = self.store();
        let directory =
            crate::world::cells::CellGridSources::load(&mut store, &key(0x100), Default::default())
                .unwrap();
        let p = directory.request_persistent().unwrap();
        let mut plans = vec![
            directory
                .prepare_persistent(&mut store, &p, &self.mounts, Default::default())
                .unwrap(),
        ];
        let request = directory.request_set(&[[0, 0], [1, 0]]).unwrap();
        let set = directory
            .prepare_cells(&mut store, &request, &self.mounts, Default::default())
            .unwrap();
        plans.extend((0..set.requests().len()).map(|i| set.plan(i).unwrap().clone()));
        plans
    }
    // Door prefetch tests use this helper; those modules are outside the direct residency import.
    #[allow(dead_code)]
    pub fn destination(&self, store: &mut RecordStore) -> crate::world::doors::DoorDestination {
        crate::world::doors::DoorDestination::load(
            store,
            &key(0x201),
            &key(0x310),
            Default::default(),
        )
        .unwrap()
    }
    pub fn patch(&mut self, bytes: &[u8]) {
        let body = [
            field(
                b"HEDR",
                &[1.34f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
            ),
            field(b"MAST", b"Base.esm\0"),
            field(b"DATA", &[0; 8]),
        ]
        .concat();
        fs::write(
            self.root.path().join("Data/Patch.esp"),
            [record(b"TES4", 0, 0, &body), bytes.to_vec()].concat(),
        )
        .unwrap();
        self.names.push("Patch.esp".into());
    }
}
pub(in crate::world) fn until(mut complete: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !complete() {
        assert!(
            Instant::now() < deadline,
            "connected source operation did not finish"
        );
        thread::sleep(Duration::from_millis(1));
    }
}
