#![allow(dead_code)]
use fallout_data::{
    identity::{FormKey, ProfileId},
    loaded_scripts::{Catalogue, Handle, Limits},
    plugin,
    store::RecordStore,
};
use std::{fs, path::Path};

pub fn form(local_id: u32) -> FormKey {
    FormKey {
        profile: ProfileId::NvOriginal,
        origin_plugin: "falloutnv.esm".into(),
        local_id,
    }
}
pub fn field(kind: &[u8; 4], bytes: &[u8]) -> Vec<u8> {
    [kind.as_slice(), &(bytes.len() as u16).to_le_bytes(), bytes].concat()
}
pub fn record(kind: &[u8; 4], form: u32, flags: u32, payload: &[u8]) -> Vec<u8> {
    [
        kind.as_slice(),
        &(payload.len() as u32).to_le_bytes(),
        &flags.to_le_bytes(),
        &form.to_le_bytes(),
        &[0; 8],
        payload,
    ]
    .concat()
}
pub fn header(masters: &[&str]) -> Vec<u8> {
    let mut body = field(
        b"HEDR",
        &[1.34_f32.to_le_bytes().as_slice(), &[0; 8]].concat(),
    );
    for master in masters {
        body.extend(field(b"MAST", &[master.as_bytes(), &[0]].concat()));
        body.extend(field(b"DATA", &[0; 8]));
    }
    record(b"TES4", 0, 0, &body)
}
pub fn unit(variables: &[(u32, u8)], references: &[(&[u8; 4], u32)]) -> Vec<u8> {
    // BEGIN GameMode plus END. The jump word remains unexecuted source data.
    let compiled = [0x10, 0, 6, 0, 0, 0, 4, 0, 0, 0, 0x11, 0, 0, 0];
    let mut schr = [0; 20];
    schr[4..8].copy_from_slice(&(references.len() as u32).to_le_bytes());
    schr[8..12].copy_from_slice(&(compiled.len() as u32).to_le_bytes());
    schr[12..16].copy_from_slice(&(variables.len() as u32).to_le_bytes());
    let mut body = field(b"SCHR", &schr);
    body.extend(field(b"SCDA", &compiled));
    for (index, kind) in variables {
        let mut slsd = [0; 24];
        slsd[..4].copy_from_slice(&index.to_le_bytes());
        slsd[16] = *kind;
        body.extend(field(b"SLSD", &slsd));
        body.extend(field(b"SCVR", format!("local_{index}\0").as_bytes()));
    }
    for (kind, value) in references {
        body.extend(field(kind, &value.to_le_bytes()));
    }
    body
}
pub fn write_fixture(path: &Path, unsupported: bool) {
    let mut variables = vec![(2, 1), (42, 0), (90, 0), (42, 1)];
    if unsupported {
        variables.extend([(99, 7), (0, 0)]);
    }
    let body = unit(
        &variables,
        &[
            (b"SCRV", 90),
            (b"SCRO", 0x100),
            (b"SCRO", 0x14),
            (b"SCRO", 0),
            (b"SCRO", 0x777),
            (b"SCRO", 0x200),
        ],
    );
    fs::write(
        path.join("FalloutNV.esm"),
        [
            header(&[]),
            record(b"SCPT", 0x300, 0, &body),
            record(b"ACTI", 0x100, 0, &[]),
            record(b"ACTI", 0x200, plugin::DELETED, &[]),
        ]
        .concat(),
    )
    .unwrap();
    fs::write(path.join("Other.esm"), header(&[])).unwrap();
}
pub fn load(path: &Path, order: &[&str]) -> Catalogue {
    let mut store = RecordStore::open_nv_headers(
        path,
        &order.iter().map(|s| (*s).into()).collect::<Vec<_>>(),
        plugin::Limits::default(),
    )
    .unwrap();
    Catalogue::load(&mut store, Limits::default(), |_, _| Ok(())).unwrap()
}
pub fn definition(catalogue: &Catalogue) -> Handle {
    catalogue.iter().next().unwrap().1.handle().clone()
}
