//! Exact alternate-texture declarations for one explicit equipment model role.
//! This wrapper constructs the existing equipment producer; report input cannot
//! select a model, lend mutable equipment authority or apply a material.
use super::{Sex, equipment};
use crate::{
    Error, Result, actors,
    assets::ArchiveAssets,
    identity::FormKey,
    inventory, plugin,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io::Write};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_sources: usize,
    pub max_record_bytes: usize,
    pub max_decoded_bytes: usize,
    pub max_field_visits: usize,
    pub max_fields: usize,
    pub max_arrays: usize,
    pub max_entries: usize,
    pub max_name_bytes: usize,
    pub max_raw_bytes: usize,
    pub max_bindings: usize,
    pub max_headers: usize,
    pub max_projection_bytes: usize,
    pub equipment: equipment::Limits,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_record_bytes: 1024 * 1024,
            max_decoded_bytes: 8 * 1024 * 1024,
            max_field_visits: 200_000,
            max_fields: 65_536,
            max_arrays: 256,
            max_entries: 4096,
            max_name_bytes: 1024 * 1024,
            max_raw_bytes: 2 * 1024 * 1024,
            max_bindings: 4096,
            max_headers: 4096,
            max_projection_bytes: 16 * 1024 * 1024,
            equipment: equipment::Limits::default(),
        }
    }
}
#[derive(Debug, Serialize)]
pub struct SourceHeader {
    pub key: FormKey,
    pub source_name: String,
    pub source_bytes: u64,
    pub source_sha256: String,
    pub header: plugin::RecordHeader,
}
#[derive(Debug, Serialize)]
pub struct Entry {
    pub ordinal: usize,
    pub field_byte_offset: u32,
    pub name_byte_offset: u32,
    pub name_bytes: Vec<u8>,
    pub texture_byte_offset: u32,
    pub index_byte_offset: u32,
    pub index_word: u32,
    pub mesh_index: i32,
    pub texture: inventory::Binding,
    pub texture_source: Option<SourceHeader>,
    pub schema_kind_allowed: Option<bool>,
    pub duplicate_mesh_declaration: bool,
    pub source_binding_available: bool,
}
#[derive(Debug, Serialize)]
pub struct Array {
    pub source_index: usize,
    pub field_index: usize,
    pub kind: [u8; 4],
    pub decoded_offset: u32,
    pub sha256: String,
    pub raw_bytes: Vec<u8>,
    pub declared_count: u32,
    pub entries: Vec<Entry>,
}
#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub field_visits: usize,
    pub fields: usize,
    pub arrays: usize,
    pub entries: usize,
    pub name_bytes: usize,
    pub raw_bytes: usize,
    pub bindings: usize,
    pub headers: usize,
    pub decoded_bytes: usize,
}
/// Only request constructs this authority. The old public report remains
/// encapsulated and immutable; no Deserialize or mutable accessor is provided.
#[derive(Serialize)]
pub struct Manifest<'a> {
    equipment: equipment::Manifest<'a>,
    selected_source_index: Option<usize>,
    selected_model_kind: Option<[u8; 4]>,
    alternate_kind: Option<[u8; 4]>,
    model_field_indices: Vec<usize>,
    selected_model_unique: bool,
    alternate_array_repeated: bool,
    arrays: Vec<Array>,
    counts: Counts,
    issues: Vec<&'static str>,
    texture_swaps_applied: bool,
    mesh_target_selected: bool,
    render_material_supported: bool,
    scope: &'static str,
}
impl<'a> Manifest<'a> {
    pub fn equipment(&self) -> &equipment::Manifest<'a> {
        &self.equipment
    }
    pub fn arrays(&self) -> &[Array] {
        &self.arrays
    }
}
fn budget(ok: bool, label: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "actor material override {label} budget exceeded"
        )))
    }
}
fn add(v: &mut usize, n: usize, max: usize, label: &str) -> Result<()> {
    *v = v
        .checked_add(n)
        .ok_or_else(|| Error::Unsupported(format!("actor material override {label} overflow")))?;
    budget(*v <= max, label)
}
fn position(at: usize) -> Result<u32> {
    u32::try_from(at).map_err(|_| Error::Unsupported("actor material offset exceeds u32".into()))
}
fn word(raw: &[u8], at: usize) -> Result<u32> {
    let end = at
        .checked_add(4)
        .ok_or_else(|| Error::Unsupported("actor material word offset overflow".into()))?;
    let bytes = raw
        .get(at..end)
        .ok_or_else(|| Error::Resolution("actor material alternate array word truncated".into()))?;
    Ok(u32::from_le_bytes(
        bytes.try_into().expect("four checked bytes"),
    ))
}
fn mapping(role: equipment::Role) -> Option<([u8; 4], [u8; 4])> {
    use equipment::Role;
    Some(match role {
        Role::ArmorBiped { sex: Sex::Male } => (*b"MODL", *b"MODS"),
        Role::ArmorBiped { sex: Sex::Female } => (*b"MOD3", *b"MO3S"),
        Role::ArmorWorld { sex: Sex::Male } | Role::WeaponShell => (*b"MOD2", *b"MO2S"),
        Role::ArmorWorld { sex: Sex::Female } | Role::WeaponWorld => (*b"MOD4", *b"MO4S"),
        Role::WeaponModel { mod_mask: 0 } | Role::WeaponFirstPerson { .. } => (*b"MODL", *b"MODS"),
        Role::WeaponScope => (*b"MOD3", *b"MO3S"),
        Role::WeaponModel { .. } => return None,
    })
}
fn header(
    store: &RecordStore,
    receipts: &[SourceReceipt],
    binding: &inventory::Binding,
    counts: &mut Counts,
    limits: Limits,
) -> Result<Option<SourceHeader>> {
    let Some(key) = &binding.key else {
        return Ok(None);
    };
    let Some(at) = store.winner(key) else {
        return Ok(None);
    };
    add(&mut counts.headers, 1, limits.max_headers, "header")?;
    let s = receipts
        .get(at.plugin)
        .ok_or_else(|| Error::Resolution("actor material source receipt absent".into()))?;
    let h = &store.definition(at).header;
    let t = binding
        .target
        .as_ref()
        .ok_or_else(|| Error::Resolution("actor material texture target absent".into()))?;
    if t.kind != h.kind
        || t.record_file_offset != h.offset
        || t.record_flags != h.flags
        || t.source_plugin != s.source_name
    {
        return Err(Error::Resolution(
            "actor material texture winning header differs".into(),
        ));
    }
    Ok(Some(SourceHeader {
        key: key.clone(),
        source_name: s.source_name.clone(),
        source_bytes: s.source_bytes,
        source_sha256: s.source_sha256.clone(),
        header: h.clone(),
    }))
}
struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        if b.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other("actor material projection budget"));
        }
        self.bytes += b.len();
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Independent existing-equipment consumer, never a mutable report ingress.
pub fn request<'a>(
    store: &mut RecordStore,
    actors: &'a actors::Catalogue<'a>,
    actor: &FormKey,
    choice: equipment::Choice,
    assets: &ArchiveAssets,
    limits: Limits,
) -> Result<Manifest<'a>> {
    budget(store.indices().len() <= limits.max_sources, "source")?;
    let mut equipment_limits = limits.equipment;
    equipment_limits.max_record_bytes = equipment_limits
        .max_record_bytes
        .min(limits.max_record_bytes);
    equipment_limits.max_decoded_bytes = equipment_limits
        .max_decoded_bytes
        .min(limits.max_decoded_bytes);
    equipment_limits.max_fields = equipment_limits.max_fields.min(limits.max_fields);
    equipment_limits.max_visits = equipment_limits.max_visits.min(limits.max_field_visits);
    let equipment = equipment::request(store, actors, actor, choice, assets, equipment_limits)?;
    budget(equipment.sources.len() <= limits.max_sources, "source")?;
    let receipts = store.source_receipts()?;
    let role = equipment.explicit_choice.role;
    let pair = mapping(role);
    let selected_source_index = if matches!(role, equipment::Role::WeaponFirstPerson { .. }) {
        match equipment.selected_links.as_slice() {
            [l] if !l.ambiguous_source => l.target_source_index,
            _ => None,
        }
    } else {
        equipment
            .source_records
            .first()
            .filter(|d| d.record().is_some())
            .map(|_| 0)
    };
    let counts = Counts {
        fields: equipment.counts.fields,
        field_visits: equipment.counts.visits,
        decoded_bytes: equipment.counts.decoded_bytes,
        ..Counts::default()
    };
    let mut m = Manifest {
        equipment,
        selected_source_index,
        selected_model_kind: pair.map(|p| p.0),
        alternate_kind: pair.map(|p| p.1),
        model_field_indices: Vec::new(),
        selected_model_unique: false,
        alternate_array_repeated: false,
        arrays: Vec::new(),
        counts,
        issues: Vec::new(),
        texture_swaps_applied: false,
        mesh_target_selected: false,
        render_material_supported: false,
        scope: "Explicit existing equipment model role and physical alternate-texture arrays with raw length-prefixed mesh names, signed indices and winning TXST header requests. Exact unordered role mapping and duplicate ambiguity; no model/name fallback, mod palette inheritance, texture-set body import, archive precedence, mesh selection, shader/material mutation or equip/gameplay choice",
    };
    if pair.is_none() {
        m.issues.push("modded_model_alternate_role_unavailable");
    }
    if selected_source_index.is_none() {
        m.issues.push("selected_model_source_unavailable");
    }
    if let (Some(source_index), Some((model_kind, alternate_kind))) = (selected_source_index, pair)
    {
        let d = m
            .equipment
            .source_records
            .get(source_index)
            .ok_or_else(|| {
                Error::Resolution("actor material selected equipment source absent".into())
            })?;
        let r = d.record().ok_or_else(|| {
            Error::Resolution("actor material retained equipment body absent".into())
        })?;
        // The existing producer admits version15 and exact canonical source.
        // Never add another version route or substitute a different model role.
        if r.header != d.header || r.header.version != 15 {
            return Err(Error::Resolution(
                "actor material selected source header differs".into(),
            ));
        }
        add(
            &mut m.counts.field_visits,
            d.fields.len(),
            limits.max_field_visits,
            "field visit",
        )?;
        m.model_field_indices = d
            .fields
            .iter()
            .enumerate()
            .filter_map(|(i, f)| (f.kind == model_kind).then_some(i))
            .collect();
        m.selected_model_unique = m.model_field_indices.len() == 1;
        if !m.selected_model_unique {
            m.issues.push("unique_selected_model_field_unavailable");
        }
        let at = store
            .winner(&d.key)
            .ok_or_else(|| Error::Resolution("actor material selected winner absent".into()))?;
        let mut seen = 0;
        let mut bindings = inventory::Counts::default();
        plugin::visit_subrecords(r, &d.source.plugin, |f| {
            add(
                &mut m.counts.field_visits,
                1,
                limits.max_field_visits,
                "field visit",
            )?;
            let index = seen;
            seen += 1;
            let cached = d.fields.get(index).ok_or_else(|| {
                Error::Resolution("actor material retained physical field absent".into())
            })?;
            if cached.kind != f.kind
                || cached.decoded_offset as usize != f.payload_offset
                || cached.bytes != f.data.len()
                || cached.sha256 != format!("{:x}", Sha256::digest(f.data))
            {
                return Err(Error::Resolution(
                    "actor material retained physical field differs".into(),
                ));
            }
            if f.kind != alternate_kind {
                return Ok(());
            }
            add(&mut m.counts.arrays, 1, limits.max_arrays, "array")?;
            add(
                &mut m.counts.raw_bytes,
                f.data.len(),
                limits.max_raw_bytes,
                "raw byte",
            )?;
            let count = word(f.data, 0)?;
            let count_usize = usize::try_from(count)
                .map_err(|_| Error::Unsupported("actor material count exceeds usize".into()))?;
            add(
                &mut m.counts.entries,
                count_usize,
                limits.max_entries,
                "entry",
            )?;
            let min = count_usize
                .checked_mul(12)
                .and_then(|n| n.checked_add(4))
                .ok_or_else(|| Error::Unsupported("actor material entry extent overflow".into()))?;
            if min > f.data.len() {
                return Err(Error::Resolution(
                    "actor material alternate array count exceeds physical extent".into(),
                ));
            }
            let mut array = Array {
                source_index,
                field_index: index,
                kind: f.kind,
                decoded_offset: cached.decoded_offset,
                sha256: cached.sha256.clone(),
                raw_bytes: f.data.to_vec(),
                declared_count: count,
                entries: Vec::new(),
            };
            let mut cursor = 4usize;
            for ordinal in 0..count_usize {
                add(
                    &mut m.counts.field_visits,
                    1,
                    limits.max_field_visits,
                    "field visit",
                )?;
                let start = cursor;
                let size = usize::try_from(word(f.data, cursor)?).map_err(|_| {
                    Error::Unsupported("actor material name length exceeds usize".into())
                })?;
                cursor = cursor.checked_add(4).ok_or_else(|| {
                    Error::Unsupported("actor material name offset overflow".into())
                })?;
                let name_at = cursor;
                let name_end = cursor.checked_add(size).ok_or_else(|| {
                    Error::Unsupported("actor material name extent overflow".into())
                })?;
                let name = f.data.get(cursor..name_end).ok_or_else(|| {
                    Error::Resolution("actor material mesh name frame truncated".into())
                })?;
                add(
                    &mut m.counts.name_bytes,
                    size,
                    limits.max_name_bytes,
                    "name byte",
                )?;
                cursor = name_end;
                let texture_at = cursor;
                let texture_word = word(f.data, cursor)?;
                cursor = cursor.checked_add(4).ok_or_else(|| {
                    Error::Unsupported("actor material texture offset overflow".into())
                })?;
                let index_at = cursor;
                let index_word = word(f.data, cursor)?;
                cursor = cursor.checked_add(4).ok_or_else(|| {
                    Error::Unsupported("actor material index offset overflow".into())
                })?;
                add(&mut m.counts.bindings, 1, limits.max_bindings, "binding")?;
                let texture = inventory::binding(store, at, texture_word, &mut bindings)?;
                let allowed = texture.target.as_ref().map(|t| t.kind == *b"TXST");
                let texture_source = header(store, &receipts, &texture, &mut m.counts, limits)?;
                let admitted = m.selected_model_unique
                    && texture.status == inventory::Status::Defined
                    && allowed == Some(true);
                array.entries.push(Entry {
                    ordinal,
                    field_byte_offset: position(start)?,
                    name_byte_offset: position(name_at)?,
                    name_bytes: name.to_vec(),
                    texture_byte_offset: position(texture_at)?,
                    index_byte_offset: position(index_at)?,
                    index_word,
                    mesh_index: index_word as i32,
                    texture,
                    texture_source,
                    schema_kind_allowed: allowed,
                    duplicate_mesh_declaration: false,
                    source_binding_available: admitted,
                });
            }
            if cursor != f.data.len() {
                return Err(Error::Resolution(
                    "actor material alternate array trailing bytes".into(),
                ));
            }
            m.arrays.push(array);
            Ok(())
        })?;
        if seen != d.fields.len() {
            return Err(Error::Resolution(
                "actor material physical field count differs".into(),
            ));
        }
        m.alternate_array_repeated = m.arrays.len() > 1;
        if m.arrays.is_empty() {
            m.issues.push("selected_alternate_array_absent");
        }
        if m.alternate_array_repeated {
            m.issues.push("repeated_selected_alternate_array");
        }
        // Borrow exact byte frames as keys. No decoded-name matching, editor
        // sorting or quadratic pair scan can alter or expand the declaration.
        add(
            &mut m.counts.field_visits,
            m.counts.entries,
            limits.max_field_visits,
            "field visit",
        )?;
        let mut occurrences = BTreeMap::<(&[u8], i32), usize>::new();
        for a in &m.arrays {
            for e in &a.entries {
                *occurrences
                    .entry((&e.name_bytes, e.mesh_index))
                    .or_default() += 1;
            }
        }
        add(
            &mut m.counts.field_visits,
            m.counts.entries,
            limits.max_field_visits,
            "field visit",
        )?;
        let duplicates = m
            .arrays
            .iter()
            .flat_map(|a| &a.entries)
            .map(|e| {
                occurrences
                    .get(&(e.name_bytes.as_slice(), e.mesh_index))
                    .is_some_and(|n| *n > 1)
            })
            .collect::<Vec<_>>();
        drop(occurrences);
        add(
            &mut m.counts.field_visits,
            m.counts.entries,
            limits.max_field_visits,
            "field visit",
        )?;
        for (e, duplicate) in m
            .arrays
            .iter_mut()
            .flat_map(|a| &mut a.entries)
            .zip(duplicates)
        {
            e.duplicate_mesh_declaration = duplicate;
            if m.alternate_array_repeated || e.duplicate_mesh_declaration {
                e.source_binding_available = false;
            }
        }
    }
    serde_json::to_writer(
        &mut Admission {
            bytes: 0,
            maximum: limits.max_projection_bytes,
        },
        &m,
    )
    .map_err(|e| Error::Unsupported(format!("actor material override projection: {e}")))?;
    Ok(m)
}
