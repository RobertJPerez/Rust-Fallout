//! Exact existing actor/race/class source inputs, without applying initialization.
use super::{Catalogue, Definition, associations, classes, races};
use crate::{
    Error, Result,
    identity::FormKey,
    inventory, plugin, record_metadata,
    store::{RecordStore, SourceReceipt},
};
use serde::Serialize;
use std::{collections::BTreeMap, io::Write};

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_sources: usize,
    pub max_field_visits: usize,
    pub max_selected_fields: usize,
    pub max_links: usize,
    pub max_decoded_bytes: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_sources: 256,
            max_field_visits: 200_000,
            max_selected_fields: 65_536,
            max_links: 4096,
            max_decoded_bytes: 8 * 1024 * 1024,
            max_projection_bytes: 16 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Serialize)]
pub struct Link<'a> {
    pub association: &'a associations::Association,
    pub field: &'a super::fields::Field,
    pub raw_bytes: Vec<u8>,
    pub repeated: bool,
    pub race_definition: Option<&'a races::Definition>,
    pub class_definition: Option<&'a classes::Definition>,
    /// Direct authored link only; never an effective or inherited selection.
    pub direct_binding_available: bool,
    pub initialization_inputs_available: bool,
    pub issues: Vec<&'static str>,
}
/// Only request() constructs this authority; serialized reports cannot recreate it.
#[derive(Debug, Serialize)]
pub struct Manifest<'a> {
    sources: &'a [SourceReceipt],
    winning_content_sha256: &'a str,
    actor: &'a Definition<'a>,
    configuration_fields: Vec<&'a inventory::Field>,
    traits_template_flag: Option<bool>,
    links: Vec<Link<'a>>,
    selected_fields: usize,
    field_visits: usize,
    decoded_bytes: usize,
    issues: Vec<&'static str>,
    initialization_supported: bool,
    scope: &'static str,
}
impl<'a> Manifest<'a> {
    pub fn sources(&self) -> &[SourceReceipt] {
        self.sources
    }
    pub fn winning_content_sha256(&self) -> &str {
        self.winning_content_sha256
    }
    pub fn actor(&self) -> &Definition<'a> {
        self.actor
    }
    pub fn links(&self) -> &[Link<'a>] {
        &self.links
    }
}
fn capacity(ok: bool, label: &str) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(Error::Unsupported(format!(
            "actor initialization input {label} budget exceeded"
        )))
    }
}
fn add(value: &mut usize, amount: usize, maximum: usize, label: &str) -> Result<()> {
    *value = value.checked_add(amount).ok_or_else(|| {
        Error::Unsupported(format!("actor initialization input {label} overflow"))
    })?;
    capacity(*value <= maximum, label)
}
fn same_sources(a: &[SourceReceipt], b: &[SourceReceipt]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.source_name == b.source_name
                && a.source_bytes == b.source_bytes
                && a.source_sha256 == b.source_sha256
        })
}
fn winning_source(
    store: &RecordStore,
    receipts: &[SourceReceipt],
    key: &FormKey,
    source: &inventory::Source,
    header: &plugin::RecordHeader,
) -> Result<()> {
    let at = store
        .winner(key)
        .ok_or_else(|| Error::Resolution("actor initialization input winner unavailable".into()))?;
    let actual = &store.definition(at).header;
    let receipt = receipts
        .get(at.plugin)
        .ok_or_else(|| Error::Resolution("actor initialization input source unavailable".into()))?;
    if serde_json::to_value(actual).map_err(|e| Error::Resolution(e.to_string()))?
        != serde_json::to_value(header).map_err(|e| Error::Resolution(e.to_string()))?
        || store.source_name(at) != source.plugin
        || receipt.source_name != source.plugin
        || receipt.source_sha256 != source.sha256
        || source.record_file_offset != header.offset
        || source.record_flags != header.flags
    {
        return Err(Error::Resolution(
            "actor initialization input retained winner differs".into(),
        ));
    }
    Ok(())
}
struct Admission {
    bytes: usize,
    maximum: usize,
}
impl Write for Admission {
    fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
        if raw.len() > self.maximum.saturating_sub(self.bytes) {
            return Err(std::io::Error::other(
                "actor initialization input projection budget",
            ));
        }
        self.bytes += raw.len();
        Ok(raw.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Protected source digest/cache observation; no decoder, source or runtime write.
pub fn request<'a>(
    store: &mut RecordStore,
    actors: &'a Catalogue<'a>,
    associations: &'a associations::Catalogue<'a>,
    races: &'a races::Catalogue,
    classes: &'a classes::Catalogue,
    root: &FormKey,
    limits: Limits,
) -> Result<Manifest<'a>> {
    capacity(store.indices().len() <= limits.max_sources, "source")?;
    let receipts = store.source_receipts()?;
    let winners = record_metadata::inspect(store)?.winning_definitions_sha256;
    for (sources, digest) in [
        (actors.sources(), actors.winning_content_sha256()),
        (
            associations.sources(),
            associations.winning_content_sha256(),
        ),
        (races.sources(), races.winning_content_sha256()),
        (classes.sources(), classes.winning_content_sha256()),
    ] {
        capacity(sources.len() <= limits.max_sources, "source")?;
        if !same_sources(sources, &receipts) || digest != winners {
            return Err(Error::Resolution(
                "actor initialization input source cohorts differ".into(),
            ));
        }
    }
    let actor = actors.get(root).filter(|a| !a.deleted).ok_or_else(|| {
        Error::Resolution("actor initialization input root unavailable or deleted".into())
    })?;
    let record = actor.record().ok_or_else(|| {
        Error::Resolution("actor initialization input retained actor unavailable".into())
    })?;
    winning_source(store, &receipts, root, actor.source, &record.header)?;
    let joined = associations.get(root).ok_or_else(|| {
        Error::Resolution("actor initialization input association root unavailable".into())
    })?;
    let inventory_fields = &actor.inventory_definition().fields;
    let mut visits = 0;
    add(
        &mut visits,
        actor.fields.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    add(
        &mut visits,
        actor.fields.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    add(
        &mut visits,
        inventory_fields.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    add(
        &mut visits,
        joined.associations.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    add(
        &mut visits,
        joined.associations.len(),
        limits.max_field_visits,
        "field visit",
    )?;
    let mut selected_fields = actor.fields.len();
    capacity(
        selected_fields <= limits.max_selected_fields,
        "selected field",
    )?;
    let decoded_bytes = record.payload.len();
    capacity(decoded_bytes <= limits.max_decoded_bytes, "decoded byte")?;
    let configuration_fields: Vec<_> = inventory_fields
        .iter()
        .filter(|f| f.kind == *b"ACBS")
        .collect();
    add(
        &mut selected_fields,
        configuration_fields.len(),
        limits.max_selected_fields,
        "selected field",
    )?;
    let traits_template_flag = match configuration_fields.as_slice() {
        [f] => match f.value {
            inventory::Value::ActorBase { template_flags, .. } => Some(template_flags & 1 != 0),
            _ => None,
        },
        _ => None,
    };
    let mut result = Manifest {
        sources: actors.sources(),
        winning_content_sha256: actors.winning_content_sha256(),
        actor,
        configuration_fields,
        traits_template_flag,
        links: Vec::new(),
        selected_fields,
        field_visits: visits,
        decoded_bytes,
        issues: Vec::new(),
        initialization_supported: false,
        scope: "Exact physical NPC RNAM/CNAM links and existing RACE/CLAS scalar declarations; both sex arrays/raw signed/float bits, no effective selection, template inheritance, current actor values, default or initialization formula",
    };
    if actor.kind != *b"NPC_" {
        result
            .issues
            .push("race_class_roles_not_applicable_to_creature");
    }
    if traits_template_flag.is_none() {
        result.issues.push("unique_configuration_unavailable");
    }
    if traits_template_flag == Some(true) {
        result
            .issues
            .push("traits_template_inheritance_unsupported");
    }
    let mut counts = [0usize; 2];
    for link in &joined.associations {
        match link.role {
            associations::Role::Race => counts[0] += 1,
            associations::Role::Class => counts[1] += 1,
            _ => (),
        }
    }
    capacity(
        counts[0].saturating_add(counts[1]) <= limits.max_links,
        "link",
    )?;
    if actor.kind == *b"NPC_" {
        if counts[0] == 0 {
            result.issues.push("missing_race_link");
        }
        if counts[1] == 0 {
            result.issues.push("missing_class_link");
        }
    }
    let mut physical_indices = BTreeMap::new();
    for association in &joined.associations {
        let (role_index, kind) = match association.role {
            associations::Role::Race => (0, *b"RACE"),
            associations::Role::Class => (1, *b"CLAS"),
            _ => continue,
        };
        if actor.kind != *b"NPC_" {
            return Err(Error::Resolution(
                "actor initialization input association kind differs".into(),
            ));
        }
        let field = actor.fields.get(association.field_index).ok_or_else(|| {
            Error::Resolution("actor initialization input physical association missing".into())
        })?;
        if field.kind != if role_index == 0 { *b"RNAM" } else { *b"CNAM" } {
            return Err(Error::Resolution(
                "actor initialization input physical association kind differs".into(),
            ));
        }
        add(
            &mut result.selected_fields,
            1,
            limits.max_selected_fields,
            "selected field",
        )?;
        let mut link = Link {
            association,
            field,
            raw_bytes: Vec::new(),
            repeated: counts[role_index] > 1,
            race_definition: None,
            class_definition: None,
            direct_binding_available: false,
            initialization_inputs_available: false,
            issues: Vec::new(),
        };
        if link.repeated {
            link.issues.push("repeated_initialization_link");
        }
        if field.bytes != 4 {
            link.issues.push("unsupported_initialization_link_layout");
        }
        match association.binding.status {
            inventory::Status::Null => link.issues.push("null_initialization_link"),
            inventory::Status::Missing => link.issues.push("missing_initialization_target"),
            inventory::Status::Deleted => link.issues.push("deleted_initialization_target"),
            inventory::Status::Defined => (),
        }
        if association.schema_kind_allowed == Some(false) {
            link.issues.push("initialization_target_kind_not_allowed");
        }
        let mut decoded_available = false;
        if let (Some(key), Some(target)) = (&association.binding.key, &association.binding.target)
            && target.kind == kind
        {
            let (source, header, fields, bytes, available) = if role_index == 0 {
                let definition = races.get(key).ok_or_else(|| {
                    Error::Resolution("actor initialization input race producer missing".into())
                })?;
                link.race_definition = Some(definition);
                (
                    &definition.source,
                    &definition.header,
                    definition.fields.len(),
                    definition.record().map_or(0, |r| r.payload.len()),
                    !definition.deleted && definition.findings.is_empty(),
                )
            } else {
                let definition = classes.get(key).ok_or_else(|| {
                    Error::Resolution("actor initialization input class producer missing".into())
                })?;
                link.class_definition = Some(definition);
                (
                    &definition.source,
                    &definition.header,
                    definition.fields.len(),
                    definition.record().map_or(0, |r| r.payload.len()),
                    !definition.deleted && definition.findings.is_empty(),
                )
            };
            winning_source(store, &receipts, key, source, header)?;
            if target.source_plugin != source.plugin
                || target.record_file_offset != header.offset
                || target.record_flags != header.flags
            {
                return Err(Error::Resolution(
                    "actor initialization input bound target differs".into(),
                ));
            }
            add(
                &mut result.selected_fields,
                fields,
                limits.max_selected_fields,
                "selected field",
            )?;
            add(
                &mut result.field_visits,
                fields,
                limits.max_field_visits,
                "field visit",
            )?;
            add(
                &mut result.decoded_bytes,
                bytes,
                limits.max_decoded_bytes,
                "decoded byte",
            )?;
            decoded_available = available;
            if !available {
                link.issues.push("target_scalar_inputs_unavailable");
            }
        }
        link.direct_binding_available = !link.repeated
            && field.bytes == 4
            && result.traits_template_flag == Some(false)
            && association.binding.status == inventory::Status::Defined
            && association.schema_kind_allowed == Some(true);
        link.initialization_inputs_available = link.direct_binding_available && decoded_available;
        if physical_indices
            .insert(association.field_index, result.links.len())
            .is_some()
        {
            return Err(Error::Resolution(
                "actor initialization input duplicate physical association".into(),
            ));
        }
        result.links.push(link);
    }
    let mut seen = 0;
    plugin::visit_subrecords(record, &actor.source.plugin, |field| {
        let index = seen;
        seen += 1;
        let cached = actor.fields.get(index).ok_or_else(|| {
            Error::Resolution("actor initialization input physical actor field missing".into())
        })?;
        if cached.kind != field.kind
            || cached.decoded_offset as usize != field.payload_offset
            || cached.bytes != field.data.len()
        {
            return Err(Error::Resolution(
                "actor initialization input physical actor field differs".into(),
            ));
        }
        if let Some(link) = physical_indices.get(&index) {
            result.links[*link].raw_bytes = field.data.to_vec();
        }
        Ok(())
    })?;
    if seen != actor.fields.len() {
        return Err(Error::Resolution(
            "actor initialization input physical field count differs".into(),
        ));
    }
    serde_json::to_writer(
        &mut Admission {
            bytes: 0,
            maximum: limits.max_projection_bytes,
        },
        &result,
    )
    .map_err(|e| Error::Unsupported(format!("actor initialization input projection: {e}")))?;
    Ok(result)
}
