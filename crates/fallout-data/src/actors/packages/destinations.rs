//! Physical authored location/target operands, without selecting an AI destination.
use super::{Catalogue, Definition};
use crate::{
    Error, Result,
    identity::FormKey,
    inventory, plugin, record_metadata,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_sources: usize,
    pub max_field_visits: usize,
    pub max_operands: usize,
    pub max_operand_bytes: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_field_visits: 200_000,
            max_operands: 4096,
            max_operand_bytes: 1024 * 1024,
            max_projection_bytes: 8 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Alternative {
    Reference,
    Cell,
    ObjectId,
    ObjectType,
    Unused,
    Unknown,
    UnsupportedLayout,
}
#[derive(Debug, Serialize)]
pub struct Operand {
    pub field_index: usize,
    pub field_kind: [u8; 4],
    pub field_decoded_offset: u32,
    pub raw_bytes: Vec<u8>,
    pub discriminant: Option<i32>,
    pub union_word: Option<u32>,
    /// Authored signed Radius for locations, Count / Distance for targets.
    pub signed_scalar: Option<i32>,
    pub unknown_float_bits: Option<u32>,
    pub alternative: Alternative,
    pub binding: Option<inventory::Binding>,
    pub schema_kind_allowed: Option<bool>,
    pub object_type_known: Option<bool>,
    pub repeated: bool,
    /// Only a unique explicit, live, schema-permitted source FormID operand.
    /// Admission does not choose this operand or evaluate its radius/count.
    pub binding_admitted: bool,
    pub findings: Vec<&'static str>,
}
#[derive(Debug, Serialize)]
pub struct Manifest<'a> {
    pub sources: &'a [SourceReceipt],
    pub winning_content_sha256: &'a str,
    pub package: &'a Definition,
    pub operands: Vec<Operand>,
    pub field_visits: usize,
    pub operand_bytes: usize,
    pub source_layouts_supported: bool,
    pub path_target_selected: bool,
    pub execution_supported: bool,
    pub issues: Vec<&'static str>,
    pub scope: &'static str,
}
fn budget(ok: bool, label: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "package destination {label} budget exceeded"
        )))
    }
}
fn slot(kind: &[u8; 4]) -> Option<usize> {
    match kind {
        b"PLDT" => Some(0),
        b"PLD2" => Some(1),
        b"PTDT" => Some(2),
        b"PTD2" => Some(3),
        _ => None,
    }
}
fn word(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(
        raw[at..at + 4]
            .try_into()
            .expect("admitted destination word"),
    )
}
fn allowed(alternative: Alternative, slot: usize, kind: &[u8; 4]) -> bool {
    match alternative {
        Alternative::Reference => matches!(
            kind,
            b"REFR" | b"PGRE" | b"PMIS" | b"PBEA" | b"ACHR" | b"ACRE" | b"PLYR"
        ),
        Alternative::Cell => kind == b"CELL",
        Alternative::ObjectId => {
            matches!(
                kind,
                b"ACTI"
                    | b"DOOR"
                    | b"STAT"
                    | b"FURN"
                    | b"CREA"
                    | b"SPEL"
                    | b"NPC_"
                    | b"CONT"
                    | b"ARMO"
                    | b"AMMO"
                    | b"MISC"
                    | b"WEAP"
                    | b"BOOK"
                    | b"KEYM"
                    | b"ALCH"
                    | b"LIGH"
                    | b"CHIP"
                    | b"CMNY"
                    | b"CCRD"
                    | b"IMOD"
            ) || (slot >= 2 && matches!(kind, b"LVLN" | b"LVLC" | b"FACT" | b"FLST"))
                || (slot == 2 && kind == b"IDLM")
        }
        _ => false,
    }
}
struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
        if raw.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "package destination projection budget",
            ));
        }
        self.bytes += raw.len();
        Ok(raw.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Reuses the selected catalogue's already admitted physical PACK record and
/// the store's canonical FormID binder. No plugin, script or condition parser.
pub fn request<'a>(
    store: &mut RecordStore,
    packages: &'a Catalogue,
    root: &FormKey,
    limits: Limits,
) -> Result<Manifest<'a>> {
    budget(packages.sources().len() <= limits.max_sources, "source")?;
    let receipts = store.source_receipts()?;
    if serde_json::to_value(&receipts).map_err(|e| Error::Resolution(e.to_string()))?
        != serde_json::to_value(packages.sources()).map_err(|e| Error::Resolution(e.to_string()))?
        || record_metadata::inspect(store)?.winning_definitions_sha256
            != packages.winning_content_sha256()
    {
        return Err(Error::Resolution(
            "package destination source cohort differs".into(),
        ));
    }
    let package = packages
        .get(root)
        .ok_or_else(|| Error::Resolution("package destination PACK root unavailable".into()))?;
    let at = store
        .winner(root)
        .ok_or_else(|| Error::Resolution("package destination winner unavailable".into()))?;
    if serde_json::to_value(&store.definition(at).header)
        .map_err(|e| Error::Resolution(e.to_string()))?
        != serde_json::to_value(&package.header).map_err(|e| Error::Resolution(e.to_string()))?
    {
        return Err(Error::Resolution(
            "package destination winning header differs".into(),
        ));
    }
    if package.deleted {
        return Err(Error::Resolution("package destination root deleted".into()));
    }
    let record = package
        .record()
        .ok_or_else(|| Error::Resolution("package destination record unavailable".into()))?;
    budget(
        package.fields.len() <= limits.max_field_visits,
        "field visit",
    )?;
    let mut counts = [0usize; 4];
    for field in &package.fields {
        if let Some(slot) = slot(&field.kind) {
            counts[slot] += 1;
        }
    }
    budget(
        counts.iter().sum::<usize>() <= limits.max_operands,
        "operand",
    )?;
    let mut result = Manifest {
        sources: packages.sources(),
        winning_content_sha256: packages.winning_content_sha256(),
        package,
        operands: Vec::new(),
        field_visits: 0,
        operand_bytes: 0,
        source_layouts_supported: true,
        path_target_selected: false,
        execution_supported: false,
        issues: Vec::new(),
        scope: "Exact physical PACK location/target operands; signed source words and optional float bits, unique live schema-permitted FormID bindings only; no selected destination, radius fallback, search, pathfinding, schedule/condition truth or AI execution",
    };
    let mut binding_counts = inventory::Counts::default();
    plugin::visit_subrecords(record, &package.source.plugin, |field| {
        let index = result.field_visits;
        result.field_visits += 1;
        let Some(slot) = slot(&field.kind) else {
            return Ok(());
        };
        budget(
            field.data.len()
                <= limits
                    .max_operand_bytes
                    .saturating_sub(result.operand_bytes),
            "operand byte",
        )?;
        result.operand_bytes += field.data.len();
        let physical = &package.fields[index];
        if physical.kind != field.kind
            || physical.decoded_offset as usize != field.payload_offset
            || physical.bytes != field.data.len()
        {
            return Err(Error::Resolution(
                "package destination physical field differs".into(),
            ));
        }
        let valid = if slot < 2 {
            field.data.len() == 12
        } else {
            matches!(field.data.len(), 12 | 16)
        };
        let mut operand = Operand {
            field_index: index,
            field_kind: field.kind,
            field_decoded_offset: physical.decoded_offset,
            raw_bytes: field.data.to_vec(),
            discriminant: None,
            union_word: None,
            signed_scalar: None,
            unknown_float_bits: None,
            alternative: Alternative::UnsupportedLayout,
            binding: None,
            schema_kind_allowed: None,
            object_type_known: None,
            repeated: counts[slot] > 1,
            binding_admitted: false,
            findings: Vec::new(),
        };
        if operand.repeated {
            operand.findings.push("repeated_destination_operand");
        }
        if valid {
            let discriminant = word(field.data, 0) as i32;
            let raw = word(field.data, 4);
            operand.discriminant = Some(discriminant);
            operand.union_word = Some(raw);
            operand.signed_scalar = Some(word(field.data, 8) as i32);
            if field.data.len() == 16 {
                operand.unknown_float_bits = Some(word(field.data, 12));
            }
            operand.alternative = if slot < 2 {
                match discriminant {
                    0 => Alternative::Reference,
                    1 => Alternative::Cell,
                    4 => Alternative::ObjectId,
                    5 => Alternative::ObjectType,
                    2 | 3 | 6 | 7 => Alternative::Unused,
                    _ => Alternative::Unknown,
                }
            } else {
                match discriminant {
                    0 => Alternative::Reference,
                    1 => Alternative::ObjectId,
                    2 => Alternative::ObjectType,
                    3 => Alternative::Unused,
                    _ => Alternative::Unknown,
                }
            };
            match operand.alternative {
                Alternative::Reference | Alternative::Cell | Alternative::ObjectId => {
                    let binding = inventory::binding(store, at, raw, &mut binding_counts)?;
                    operand.schema_kind_allowed = binding
                        .target
                        .as_ref()
                        .map(|target| allowed(operand.alternative, slot, &target.kind));
                    match binding.status {
                        inventory::Status::Null => operand.findings.push("null_destination_form"),
                        inventory::Status::Missing => {
                            operand.findings.push("missing_destination_form")
                        }
                        inventory::Status::Deleted => {
                            operand.findings.push("deleted_destination_form")
                        }
                        inventory::Status::Defined => {}
                    }
                    if operand.schema_kind_allowed == Some(false) {
                        operand.findings.push("destination_form_kind_not_allowed");
                    }
                    operand.binding_admitted = !operand.repeated
                        && binding.status == inventory::Status::Defined
                        && operand.schema_kind_allowed == Some(true);
                    operand.binding = Some(binding);
                }
                Alternative::ObjectType => {
                    operand.object_type_known = Some(raw <= 28);
                    if raw > 28 {
                        operand.findings.push("unknown_destination_object_type");
                    }
                }
                Alternative::Unused => operand.findings.push("destination_context_unavailable"),
                Alternative::Unknown => operand.findings.push("unknown_destination_discriminant"),
                Alternative::UnsupportedLayout => unreachable!(),
            }
        } else {
            operand.findings.push("unsupported_destination_layout");
        }
        if !valid
            || operand.alternative == Alternative::Unknown
            || operand.object_type_known == Some(false)
        {
            result.source_layouts_supported = false;
        }
        result.operands.push(operand);
        Ok(())
    })?;
    if result.operands.is_empty() {
        result.issues.push("no_authored_destination_operands");
    }
    serde_json::to_writer(
        Admission {
            bytes: 0,
            maximum: limits.max_projection_bytes,
        },
        &result,
    )
    .map_err(|_| {
        Error::Unsupported("package destination projection byte budget exceeded".into())
    })?;
    Ok(result)
}
