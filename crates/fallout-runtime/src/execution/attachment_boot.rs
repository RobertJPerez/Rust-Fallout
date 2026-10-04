//! Explicit engineering creation of one uniquely attached quest script owner.
//! Source attachment is not evidence of retail quest activation or scheduling.
use super::copy_probe::Initializer;
use crate::{
    World,
    events::Context,
    foreign::Content,
    identity::{CampaignId, InstanceId, Owner, ReferenceValue, Value},
    programs::PreparedSources,
    snapshot::Snapshot,
};
use fallout_data::{
    identity::FormKey,
    loaded_scripts::{Handle, Version},
    plugin,
    quest_scripts::{Attachment, Attachments, Status},
    store::SourceReceipt,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub campaign: CampaignId,
    pub context: Context,
    pub initializers: Vec<Initializer>,
}

/// Logical input bounds; private restoration also uses canonical World limits.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub maximum_initializers: usize,
    pub maximum_context_arguments: usize,
    pub maximum_variable_bytes: usize,
    pub maximum_source_receipt_bytes: usize,
    pub maximum_declarations: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            maximum_initializers: 128,
            maximum_context_arguments: 64,
            maximum_variable_bytes: 64 * 1024,
            maximum_source_receipt_bytes: 1024 * 1024,
            maximum_declarations: 65_536,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub initializers: usize,
    pub context_arguments: usize,
    pub variable_bytes: usize,
    /// Name and SHA UTF-8 bytes plus eight bytes for each source length.
    pub source_receipt_bytes: usize,
    pub declarations: usize,
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("quest attachment boot input is invalid: {0}")]
    Input(&'static str),
    #[error("quest attachment boot budget exceeded: {0}")]
    Capacity(&'static str),
    #[error("quest script attachment is unavailable: {0:?}")]
    Attachment(Status),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Source(#[from] crate::programs::LookupError),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
}

/// Private source-bound request, never deserialized or a live mutation token.
pub struct BootPlan<'p, 's> {
    sources: &'p PreparedSources<'s>,
    content: &'p Content,
    attachment: &'p Attachment,
    request: &'p Request,
    counts: Counts,
}
#[derive(Debug, Serialize)]
pub struct BootResult {
    pub instance: InstanceId,
    pub snapshot: Snapshot,
}

fn charge(
    used: &mut usize,
    amount: usize,
    maximum: usize,
    kind: &'static str,
) -> Result<(), Error> {
    *used = used
        .checked_add(amount)
        .filter(|&value| value <= maximum)
        .ok_or(Error::Capacity(kind))?;
    Ok(())
}
fn string_bytes(value: &ReferenceValue) -> usize {
    match value {
        ReferenceValue::Content { key } => key.origin_plugin.len(),
        _ => 0,
    }
}

pub fn prepare<'p, 's>(
    sources: &'p PreparedSources<'s>,
    attachments: &'p Attachments,
    content: &'p Content,
    explicit_quest: &FormKey,
    request: &'p Request,
    limits: Limits,
) -> Result<BootPlan<'p, 's>, Error> {
    let mut counts = prepare_input(
        sources,
        attachments.source_receipts(),
        explicit_quest,
        request,
        limits,
    )?;
    let catalogue = sources.catalogue();
    // This empty private World supplies the existing content/cohort validation
    // API. It creates no script owner, values, references or journal entries.
    let validation_world =
        World::with_campaign(catalogue, crate::Limits::default(), request.campaign)?;
    content.validate_world(&validation_world)?;
    let attachment = validated_attachment(attachments, content, &validation_world, explicit_quest)?;
    let definition = attachment
        .script
        .as_ref()
        .expect("checked exact attachment");
    counts.declarations = prepare_definition(sources, definition, limits)?;
    Ok(BootPlan {
        sources,
        content,
        attachment,
        request,
        counts,
    })
}

// Both entry points use exactly the same physical attachment validation.
fn validated_attachment<'a>(
    attachments: &'a Attachments,
    content: &Content,
    validation_world: &World<'_>,
    explicit_quest: &FormKey,
) -> Result<&'a Attachment, Error> {
    let form = content.source_form(validation_world, explicit_quest)?;
    if form.kind != *b"QUST" {
        return Err(Error::Input("explicit attachment owner is not a quest"));
    }
    let attachment = attachments
        .get(explicit_quest)
        .ok_or(Error::Input("quest attachment is missing"))?;
    if form.flags & plugin::DELETED != 0 || attachment.status != Status::LoadedDefinition {
        return Err(Error::Attachment(attachment.status));
    }
    if !attachment.findings.is_empty()
        || attachment.fields.len() != 1
        || attachment.source.decoded_record_sha256.is_none()
    {
        return Err(Error::Input("quest attachment retains source findings"));
    }
    let definition = attachment
        .script
        .as_ref()
        .ok_or(Error::Input("quest attachment lost its exact definition"))?;
    if attachment.fields[0].key.as_ref() != Some(&definition.key.record) {
        return Err(Error::Input(
            "quest attachment field differs from its definition",
        ));
    }
    Ok(attachment)
}

/// Shared bounded initializer/context/cohort validation. Retains the historical
/// quest route's validation order, error strings and logical counters.
pub(crate) fn prepare_input(
    sources: &PreparedSources<'_>,
    receipts: &[SourceReceipt],
    explicit_quest: &FormKey,
    request: &Request,
    limits: Limits,
) -> Result<Counts, Error> {
    CampaignId::from_bytes(request.campaign.bytes())?;
    if request.initializers.len() > limits.maximum_initializers {
        return Err(Error::Capacity("initializers"));
    }
    if request.context.arguments.len() > limits.maximum_context_arguments {
        return Err(Error::Capacity("context arguments"));
    }
    let catalogue = sources.catalogue();
    if receipts.len() != catalogue.sources.len() {
        return Err(Error::Input(
            "attachment source receipts differ from the prepared catalogue",
        ));
    }
    let mut counts = Counts {
        initializers: request.initializers.len(),
        context_arguments: request.context.arguments.len(),
        variable_bytes: 0,
        source_receipt_bytes: 0,
        declarations: 0,
    };
    for (actual, expected) in receipts.iter().zip(&catalogue.sources) {
        for size in [actual.source_name.len(), actual.source_sha256.len(), 8] {
            charge(
                &mut counts.source_receipt_bytes,
                size,
                limits.maximum_source_receipt_bytes,
                "source receipt bytes",
            )?;
        }
        if actual.source_name != expected.source_name
            || actual.source_bytes != expected.source_bytes
            || actual.source_sha256 != expected.source_sha256
        {
            return Err(Error::Input(
                "attachment source receipts differ from the prepared catalogue",
            ));
        }
    }
    charge(
        &mut counts.variable_bytes,
        explicit_quest.origin_plugin.len(),
        limits.maximum_variable_bytes,
        "variable bytes",
    )?;
    for value in request
        .context
        .target
        .iter()
        .chain(&request.context.arguments)
    {
        charge(
            &mut counts.variable_bytes,
            string_bytes(value),
            limits.maximum_variable_bytes,
            "variable bytes",
        )?;
    }
    let mut indices = BTreeSet::new();
    for initializer in &request.initializers {
        if !indices.insert(initializer.index) {
            return Err(Error::Input("duplicate local initializer"));
        }
        if let Value::Reference { value } = &initializer.value {
            charge(
                &mut counts.variable_bytes,
                string_bytes(value),
                limits.maximum_variable_bytes,
                "variable bytes",
            )?;
        }
    }
    // Canonical-name validation may allocate; admit all input strings first.
    crate::identity::valid_form(explicit_quest)?;
    Ok(counts)
}

pub(crate) fn prepare_definition(
    sources: &PreparedSources<'_>,
    definition: &Handle,
    limits: Limits,
) -> Result<usize, Error> {
    sources.get(definition)?;
    let script = sources
        .catalogue()
        .get_handle(definition)
        .ok_or(crate::Error::DefinitionChanged)?;
    let declarations = script.declarations().len();
    if declarations > limits.maximum_declarations {
        return Err(Error::Capacity("local declarations"));
    }
    Ok(declarations)
}

pub(crate) fn initialize(
    result: &mut World<'_>,
    definition: &Handle,
    owner: Owner,
    request: &Request,
) -> Result<InstanceId, Error> {
    let handle = result.create_instance(definition, owner, request.context.clone())?;
    let assignments: Vec<_> = request
        .initializers
        .iter()
        .map(|entry| (entry.index, entry.value.clone()))
        .collect();
    result.assign(handle, &assignments)?;
    Ok(result.instance(handle)?.id())
}

impl BootPlan<'_, '_> {
    pub fn attachment(&self) -> &Attachment {
        self.attachment
    }
    pub fn definition(&self) -> &Handle {
        self.attachment
            .script
            .as_ref()
            .expect("checked exact attachment")
    }
    pub fn script_version(&self) -> &Version {
        self.sources
            .catalogue()
            .get_handle(self.definition())
            .expect("immutable exact definition")
            .version()
    }
    pub fn counts(&self) -> Counts {
        self.counts
    }
    pub fn source_cohort_sha256(&self) -> &str {
        self.sources.source_cohort_sha256()
    }

    /// Consume an owned input into a private result. Every failure discards that
    /// restored World; a caller's live World is never borrowed for mutation.
    pub fn apply(&self, snapshot: Snapshot, limits: crate::Limits) -> Result<BootResult, Error> {
        if snapshot.campaign != self.request.campaign {
            return Err(Error::Input(
                "initializer campaign differs from the supplied snapshot",
            ));
        }
        let mut result = World::restore(self.sources.catalogue(), snapshot, limits)?;
        self.sources.validate_world(&result)?;
        self.content.validate_world(&result)?;
        let owner = Owner::Quest {
            key: self.attachment.quest.clone(),
        };
        let instance = initialize(&mut result, self.definition(), owner, self.request)?;
        Ok(BootResult {
            instance,
            snapshot: result.snapshot(),
        })
    }
}

/// Explicit caller order controls engineering creation only, never activation.
#[derive(Debug, Clone, Copy)]
pub struct Selection<'a> {
    pub quest: &'a FormKey,
    pub expected_definition: &'a Handle,
    pub initialization: &'a Request,
}
#[derive(Debug, Clone, Copy)]
pub struct ManyLimits {
    pub maximum_quests: usize,
    /// All five counters are cumulative across the complete selection.
    pub input: Limits,
    /// Conservative selected-name copy reservations, not a heap measurement.
    pub maximum_retained_variable_bytes: usize,
}
impl Default for ManyLimits {
    fn default() -> Self {
        Self {
            maximum_quests: 32,
            input: Limits::default(),
            maximum_retained_variable_bytes: 2 * 1024 * 1024,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ManyCounts {
    pub quests: usize,
    pub input: Counts,
    pub retained_variable_bytes: usize,
}
pub enum ManyPreparation<'p, 's> {
    Unsupported {
        reason: super::local_copy::Unsupported,
        detail: &'static str,
    },
    Ready(ManyBootPlan<'p, 's>),
}
/// Borrowed immutable source plans; no deserialization or live-world mutation.
pub struct ManyBootPlan<'p, 's> {
    plans: Vec<BootPlan<'p, 's>>,
    counts: ManyCounts,
}
#[derive(Debug, Serialize)]
pub struct ManyBootResult {
    /// Instance identities in explicit selection order.
    pub instances: Vec<InstanceId>,
    pub snapshot: Snapshot,
}

pub fn prepare_many<'p, 's>(
    sources: &'p PreparedSources<'s>,
    attachments: &'p Attachments,
    content: &'p Content,
    selections: &[Selection<'p>],
    intent: super::local_copy::Intent,
    limits: ManyLimits,
) -> Result<ManyPreparation<'p, 's>, Error> {
    if intent == super::local_copy::Intent::Faithful {
        return Ok(ManyPreparation::Unsupported {
            reason: super::local_copy::Unsupported::UnverifiedRetailSemantics,
            detail: "original quest activation order and constructor semantics are unverified",
        });
    }
    if selections.is_empty() {
        return Err(Error::Input("quest selection is empty"));
    }
    if selections.len() > limits.maximum_quests {
        return Err(Error::Capacity("selected quests"));
    }
    let campaign = selections[0].initialization.campaign;
    let mut counts = ManyCounts {
        quests: selections.len(),
        input: Counts {
            initializers: 0,
            context_arguments: 0,
            variable_bytes: 0,
            source_receipt_bytes: 0,
            declarations: 0,
        },
        retained_variable_bytes: 0,
    };
    // One empty validation World, separate from the single private restoration
    // in apply. It creates no owners, references, events or local values.
    let validation = World::with_campaign(sources.catalogue(), Default::default(), campaign)?;
    content.validate_world(&validation)?;
    let mut seen = BTreeSet::new();
    let mut plans = Vec::with_capacity(selections.len());
    for selection in selections {
        if selection.initialization.campaign != campaign {
            return Err(Error::Input("selected initializer campaigns differ"));
        }
        let remaining = Limits {
            maximum_initializers: limits.input.maximum_initializers - counts.input.initializers,
            maximum_context_arguments: limits.input.maximum_context_arguments
                - counts.input.context_arguments,
            maximum_variable_bytes: limits.input.maximum_variable_bytes
                - counts.input.variable_bytes,
            maximum_source_receipt_bytes: limits.input.maximum_source_receipt_bytes
                - counts.input.source_receipt_bytes,
            maximum_declarations: limits.input.maximum_declarations - counts.input.declarations,
        };
        let mut row = prepare_input(
            sources,
            attachments.source_receipts(),
            selection.quest,
            selection.initialization,
            remaining,
        )?;
        // Admit caller-supplied handle strings before comparing or retaining
        // any key. Duplicate detection borrows admitted canonical quest keys.
        for size in [
            selection.expected_definition.key.record.origin_plugin.len(),
            selection.expected_definition.version_sha256.len(),
        ] {
            charge(
                &mut row.variable_bytes,
                size,
                remaining.maximum_variable_bytes,
                "variable bytes",
            )?;
        }
        if !seen.insert(selection.quest) {
            return Err(Error::Input("duplicate selected quest"));
        }
        let attachment = validated_attachment(attachments, content, &validation, selection.quest)?;
        let definition = attachment
            .script
            .as_ref()
            .expect("checked exact attachment");
        if definition != selection.expected_definition {
            return Err(Error::Input(
                "selected definition differs from the exact quest attachment",
            ));
        }
        row.declarations = prepare_definition(sources, definition, remaining)?;
        // Reserve repeated names for owner indexes, canonical validation,
        // context/assignment copies and the returned snapshot. Handle copies
        // include the cached schema key and temporary normalization. Fixed
        // metadata is covered conservatively. World restoration/snapshot
        // admission separately bounds the complete pre-existing state.
        for _ in 0..6 {
            charge(
                &mut counts.retained_variable_bytes,
                row.variable_bytes,
                limits.maximum_retained_variable_bytes,
                "retained variable bytes",
            )?;
        }
        for _ in 0..8 {
            for size in [
                definition.key.record.origin_plugin.len(),
                definition.version_sha256.len(),
            ] {
                charge(
                    &mut counts.retained_variable_bytes,
                    size,
                    limits.maximum_retained_variable_bytes,
                    "retained variable bytes",
                )?;
            }
        }
        charge(
            &mut counts.retained_variable_bytes,
            1024,
            limits.maximum_retained_variable_bytes,
            "retained variable bytes",
        )?;
        counts.input.initializers += row.initializers;
        counts.input.context_arguments += row.context_arguments;
        counts.input.variable_bytes += row.variable_bytes;
        counts.input.source_receipt_bytes += row.source_receipt_bytes;
        counts.input.declarations += row.declarations;
        plans.push(BootPlan {
            sources,
            content,
            attachment,
            request: selection.initialization,
            counts: row,
        });
    }
    Ok(ManyPreparation::Ready(ManyBootPlan { plans, counts }))
}
impl<'p, 's> ManyBootPlan<'p, 's> {
    pub fn plans(&self) -> &[BootPlan<'p, 's>] {
        &self.plans
    }
    pub fn counts(&self) -> ManyCounts {
        self.counts
    }
    /// Restore once and publish only a complete result. A late failure drops
    /// every earlier private initialization together with this restored World.
    pub fn apply(
        &self,
        snapshot: Snapshot,
        limits: crate::Limits,
    ) -> Result<ManyBootResult, Error> {
        let first = &self.plans[0];
        if snapshot.campaign != first.request.campaign {
            return Err(Error::Input(
                "initializer campaign differs from the supplied snapshot",
            ));
        }
        let mut result = World::restore(first.sources.catalogue(), snapshot, limits)?;
        first.sources.validate_world(&result)?;
        first.content.validate_world(&result)?;
        let mut instances = Vec::with_capacity(self.plans.len());
        for plan in &self.plans {
            instances.push(initialize(
                &mut result,
                plan.definition(),
                Owner::Quest {
                    key: plan.attachment.quest.clone(),
                },
                plan.request,
            )?);
        }
        Ok(ManyBootResult {
            instances,
            snapshot: result.snapshot(),
        })
    }
}
