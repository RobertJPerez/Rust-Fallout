//! Physical AIDT and ZNAM source declarations, without live AI or combat rules.
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
    pub max_selected_fields: usize,
    pub max_raw_bytes: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_field_visits: 200_000,
            max_selected_fields: 4096,
            max_raw_bytes: 1024 * 1024,
            max_projection_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct KnownEnums {
    pub aggression: bool,
    pub confidence: bool,
    pub mood: bool,
    pub teaches: bool,
    pub assistance: bool,
    pub aggro_radius_behavior: bool,
}
impl KnownEnums {
    fn all(&self) -> bool {
        self.aggression
            && self.confidence
            && self.mood
            && self.teaches
            && self.assistance
            && self.aggro_radius_behavior
    }
}
#[derive(Debug, Serialize)]
pub struct AiData {
    pub aggression: u8,
    pub confidence: u8,
    pub energy_level: u8,
    pub responsibility: u8,
    pub mood: u8,
    pub mood_unused: [u8; 3],
    pub services_flags: u32,
    pub teaches: i8,
    pub maximum_training_level: u8,
    pub assistance: i8,
    pub aggro_radius_behavior: u8,
    pub aggro_radius: i32,
    pub known_enums: KnownEnums,
}
#[derive(Debug, Serialize)]
pub struct AiField {
    pub field_index: usize,
    pub field_decoded_offset: u32,
    pub raw_bytes: Vec<u8>,
    pub repeated: bool,
    pub value: Option<AiData>,
    pub findings: Vec<&'static str>,
}
#[derive(Debug, Serialize)]
pub struct CombatStyleField {
    pub field_index: usize,
    pub field_decoded_offset: u32,
    pub raw_bytes: Vec<u8>,
    pub repeated: bool,
    pub binding: Option<inventory::Binding>,
    pub schema_kind_allowed: Option<bool>,
    pub winning_header: Option<plugin::RecordHeader>,
    pub binding_admitted: bool,
    pub findings: Vec<&'static str>,
}
#[derive(Debug, Serialize)]
pub struct Manifest<'a> {
    pub sources: &'a [SourceReceipt],
    pub winning_content_sha256: &'a str,
    pub actor: &'a Definition<'a>,
    pub configuration_fields: Vec<&'a inventory::Field>,
    pub ai_data_template_flag: Option<bool>,
    pub traits_template_flag: Option<bool>,
    pub record_version_supported: bool,
    pub ai_data: Vec<AiField>,
    pub combat_styles: Vec<CombatStyleField>,
    /// One unambiguous raw source declaration, never evaluated live behavior.
    pub authored_ai_input_admitted: bool,
    pub field_visits: usize,
    pub raw_bytes: usize,
    pub issues: Vec<&'static str>,
    pub live_behavior_evaluated: bool,
    pub execution_supported: bool,
    pub scope: &'static str,
}
fn budget(ok: bool, label: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "actor AI input {label} budget exceeded"
        )))
    }
}
struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
        if raw.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other("actor AI input projection budget"));
        }
        self.bytes += raw.len();
        Ok(raw.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn word(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(
        raw[at..at + 4]
            .try_into()
            .expect("twenty admitted AIDT bytes"),
    )
}
fn ai(raw: &[u8]) -> AiData {
    let teaches = raw[12] as i8;
    let assistance = raw[14] as i8;
    AiData {
        aggression: raw[0],
        confidence: raw[1],
        energy_level: raw[2],
        responsibility: raw[3],
        mood: raw[4],
        mood_unused: raw[5..8].try_into().expect("three mood unused bytes"),
        services_flags: word(raw, 8),
        teaches,
        maximum_training_level: raw[13],
        assistance,
        aggro_radius_behavior: raw[15],
        aggro_radius: word(raw, 16) as i32,
        known_enums: KnownEnums {
            aggression: raw[0] <= 3,
            confidence: raw[1] <= 4,
            mood: raw[4] <= 7,
            teaches: (-1..=13).contains(&teaches),
            assistance: (0..=2).contains(&assistance),
            aggro_radius_behavior: raw[15] <= 1,
        },
    }
}

/// Existing digest/cursor cache observation only; no source or live state writes.
pub fn request<'a>(
    store: &mut RecordStore,
    actors: &'a Catalogue<'a>,
    root: &FormKey,
    limits: Limits,
) -> Result<Manifest<'a>> {
    budget(actors.sources().len() <= limits.max_sources, "source")?;
    budget(store.indices().len() <= limits.max_sources, "source")?;
    let receipts = store.source_receipts()?;
    if serde_json::to_value(&receipts).map_err(|e| Error::Resolution(e.to_string()))?
        != serde_json::to_value(actors.sources()).map_err(|e| Error::Resolution(e.to_string()))?
        || record_metadata::inspect(store)?.winning_definitions_sha256
            != actors.winning_content_sha256()
    {
        return Err(Error::Resolution(
            "actor AI input source cohort differs".into(),
        ));
    }
    let actor = actors
        .get(root)
        .ok_or_else(|| Error::Resolution("actor AI input root unavailable".into()))?;
    if actor.deleted {
        return Err(Error::Resolution("actor AI input root deleted".into()));
    }
    let at = store
        .winner(root)
        .ok_or_else(|| Error::Resolution("actor AI input winner unavailable".into()))?;
    let record = actor
        .record()
        .ok_or_else(|| Error::Resolution("actor AI input retained record unavailable".into()))?;
    let source = actors
        .sources()
        .get(at.plugin)
        .ok_or_else(|| Error::Resolution("actor AI input source unavailable".into()))?;
    if serde_json::to_value(&store.definition(at).header)
        .map_err(|e| Error::Resolution(e.to_string()))?
        != serde_json::to_value(&record.header).map_err(|e| Error::Resolution(e.to_string()))?
        || store.source_name(at) != actor.source.plugin
        || source.source_name != actor.source.plugin
        || source.source_sha256 != actor.source.sha256
        || actor.source.record_file_offset != record.header.offset
        || actor.source.record_flags != record.header.flags
    {
        return Err(Error::Resolution(
            "actor AI input winning actor differs".into(),
        ));
    }
    let inventory_fields = &actor.inventory_definition().fields;
    let visits = actor
        .fields
        .len()
        .checked_mul(2)
        .and_then(|n| n.checked_add(inventory_fields.len()))
        .ok_or_else(|| Error::Unsupported("actor AI input field visit overflow".into()))?;
    budget(visits <= limits.max_field_visits, "field visit")?;
    let (ai_count, style_count) = actor.fields.iter().fold((0, 0), |(a, s), f| {
        (
            a + usize::from(f.kind == *b"AIDT"),
            s + usize::from(f.kind == *b"ZNAM"),
        )
    });
    budget(
        ai_count + style_count <= limits.max_selected_fields,
        "selected field",
    )?;
    let configuration_fields: Vec<_> = inventory_fields
        .iter()
        .filter(|f| f.kind == *b"ACBS")
        .collect();
    let flags = match configuration_fields.as_slice() {
        [f] => match f.value {
            inventory::Value::ActorBase { template_flags, .. } => Some(template_flags),
            _ => None,
        },
        _ => None,
    };
    let supported = match actor.kind {
        k if k == *b"NPC_" => matches!(actor.record_version, Some(14 | 15)),
        k if k == *b"CREA" => matches!(actor.record_version, Some(9 | 11 | 13 | 14 | 15)),
        _ => false,
    };
    let mut result = Manifest {
        sources: actors.sources(),
        winning_content_sha256: actors.winning_content_sha256(),
        actor,
        configuration_fields,
        ai_data_template_flag: flags.map(|f| f & 16 != 0),
        traits_template_flag: flags.map(|f| f & 1 != 0),
        record_version_supported: supported,
        ai_data: Vec::new(),
        combat_styles: Vec::new(),
        authored_ai_input_admitted: false,
        field_visits: visits,
        raw_bytes: 0,
        issues: Vec::new(),
        live_behavior_evaluated: false,
        execution_supported: false,
        scope: "Exact authored AIDT words and explicit ZNAM/CSTY winning identity; raw enum/flag/signed/mood-unused source inputs only, no template inheritance, aggression/hostility/service/training/radius truth, combat-style formulas, live actor values, defaults or AI execution",
    };
    if !supported {
        result.issues.push("unsupported_actor_record_version");
    }
    if flags.is_none() {
        result.issues.push("unique_configuration_unavailable");
    }
    if result.ai_data_template_flag == Some(true) {
        result
            .issues
            .push("ai_data_template_inheritance_unsupported");
    }
    if result.traits_template_flag == Some(true) {
        result
            .issues
            .push("combat_style_template_inheritance_unsupported");
    }
    if ai_count == 0 {
        result.issues.push("missing_ai_data");
    }
    let mut count = 0;
    let mut bindings = inventory::Counts::default();
    plugin::visit_subrecords(record, &actor.source.plugin, |field| {
        let index = count;
        count += 1;
        let cached = actor
            .fields
            .get(index)
            .ok_or_else(|| Error::Resolution("actor AI input physical field missing".into()))?;
        if cached.kind != field.kind
            || cached.decoded_offset as usize != field.payload_offset
            || cached.bytes != field.data.len()
        {
            return Err(Error::Resolution(
                "actor AI input physical field differs".into(),
            ));
        }
        if !matches!(&field.kind, b"AIDT" | b"ZNAM") {
            return Ok(());
        }
        budget(
            field.data.len() <= limits.max_raw_bytes.saturating_sub(result.raw_bytes),
            "raw byte",
        )?;
        result.raw_bytes += field.data.len();
        let mut findings = Vec::new();
        if !supported {
            findings.push("unsupported_actor_record_version");
        }
        if field.kind == *b"AIDT" {
            if ai_count > 1 {
                findings.push("repeated_ai_data");
            }
            if field.data.len() != 20 {
                findings.push("unsupported_ai_data_layout");
            }
            let value = (supported && field.data.len() == 20).then(|| ai(field.data));
            if let Some(value) = &value {
                for (known, code) in [
                    (value.known_enums.aggression, "unknown_aggression_enum"),
                    (value.known_enums.confidence, "unknown_confidence_enum"),
                    (value.known_enums.mood, "unknown_mood_enum"),
                    (value.known_enums.teaches, "unknown_teaches_enum"),
                    (value.known_enums.assistance, "unknown_assistance_enum"),
                    (
                        value.known_enums.aggro_radius_behavior,
                        "unknown_aggro_radius_behavior_enum",
                    ),
                ] {
                    if !known {
                        findings.push(code);
                    }
                }
            }
            result.ai_data.push(AiField {
                field_index: index,
                field_decoded_offset: cached.decoded_offset,
                raw_bytes: field.data.to_vec(),
                repeated: ai_count > 1,
                value,
                findings,
            });
        } else {
            if style_count > 1 {
                findings.push("repeated_combat_style");
            }
            if field.data.len() != 4 {
                findings.push("unsupported_combat_style_layout");
            }
            let binding = if supported && field.data.len() == 4 {
                Some(inventory::binding(
                    store,
                    at,
                    word(field.data, 0),
                    &mut bindings,
                )?)
            } else {
                None
            };
            let allowed = binding
                .as_ref()
                .and_then(|b| b.target.as_ref())
                .map(|t| t.kind == *b"CSTY");
            if let Some(binding) = &binding {
                match binding.status {
                    inventory::Status::Null => findings.push("null_combat_style"),
                    inventory::Status::Missing => findings.push("missing_combat_style"),
                    inventory::Status::Deleted => findings.push("deleted_combat_style"),
                    inventory::Status::Defined => (),
                }
            }
            if allowed == Some(false) {
                findings.push("combat_style_kind_not_allowed");
            }
            let winning_header = binding
                .as_ref()
                .and_then(|b| b.key.as_ref())
                .and_then(|k| store.winner(k))
                .map(|at| store.definition(at).header.clone());
            let admitted = style_count == 1
                && result.traits_template_flag == Some(false)
                && allowed == Some(true)
                && binding
                    .as_ref()
                    .is_some_and(|b| b.status == inventory::Status::Defined);
            result.combat_styles.push(CombatStyleField {
                field_index: index,
                field_decoded_offset: cached.decoded_offset,
                raw_bytes: field.data.to_vec(),
                repeated: style_count > 1,
                binding,
                schema_kind_allowed: allowed,
                winning_header,
                binding_admitted: admitted,
                findings,
            });
        }
        Ok(())
    })?;
    if count != actor.fields.len() {
        return Err(Error::Resolution(
            "actor AI input field count differs".into(),
        ));
    }
    result.authored_ai_input_admitted = ai_count == 1
        && result.ai_data_template_flag == Some(false)
        && result.ai_data[0]
            .value
            .as_ref()
            .is_some_and(|v| v.known_enums.all());
    serde_json::to_writer(
        &mut Admission {
            bytes: 0,
            maximum: limits.max_projection_bytes,
        },
        &result,
    )
    .map_err(|e| Error::Unsupported(format!("actor AI input projection: {e}")))?;
    Ok(result)
}
