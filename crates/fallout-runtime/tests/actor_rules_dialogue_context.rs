mod common;
use common::*;
use fallout_data::{
    actors::{self, associations, dependencies, factions, placements},
    inventory, leveled, loaded_scripts, plugin,
    store::RecordStore,
};
use fallout_runtime::{
    Limits as WorldLimits, World,
    actor_rules::dialogue_context::{self, Choice, Sources},
    foreign::Content,
    identity::ReferenceId,
};
use std::{fs, num::NonZeroU64, path::Path};

fn reference(id: u64) -> ReferenceId {
    ReferenceId(NonZeroU64::new(id).unwrap())
}

fn disk(kind: &[u8; 4], id: u32, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut raw = record(kind, id, flags, body);
    raw[20..22].copy_from_slice(&15_u16.to_le_bytes());
    raw
}

fn actor(faction: u32, rank: i8) -> Vec<u8> {
    actor_with_template(faction, rank, 0, None)
}

fn actor_with_template(
    faction: u32,
    rank: i8,
    template_mask: u16,
    template: Option<u32>,
) -> Vec<u8> {
    let mut configuration = [0; 24];
    configuration[22..24].copy_from_slice(&template_mask.to_le_bytes());
    let mut body = [
        field(b"ACBS", &configuration),
        field(
            b"DATA",
            &[18_i32.to_le_bytes().as_slice(), &[1, 2, 3, 4, 5, 6, 7]].concat(),
        ),
        field(b"DNAM", &[0; 28]),
    ]
    .concat();
    body.extend(field(
        b"SNAM",
        &[faction.to_le_bytes().as_slice(), &[rank as u8, 0, 0, 0]].concat(),
    ));
    if let Some(template) = template {
        body.extend(field(b"TPLT", &template.to_le_bytes()));
    }
    body
}

fn placed(base: u32, marker: u32) -> Vec<u8> {
    let words = [
        marker,
        0x8000_0000,
        1,
        0x3f00_0000,
        0xbf80_0000,
        0x4000_0000,
    ];
    [
        field(b"EDID", b"DialogueActor\0"),
        field(b"NAME", &base.to_le_bytes()),
        field(
            b"DATA",
            &words
                .into_iter()
                .flat_map(u32::to_le_bytes)
                .collect::<Vec<_>>(),
        ),
    ]
    .concat()
}

fn info(speakers: &[u32]) -> Vec<u8> {
    let mut body = field(b"DATA", &[0, 0, 0]);
    for speaker in speakers {
        body.extend(field(b"ANAM", &speaker.to_le_bytes()));
    }
    body
}

fn fixture(path: &Path, speakers: &[u32]) {
    fixture_with_speaker(path, speakers, actor(0x300, 2), None);
}

fn fixture_with_speaker(
    path: &Path,
    speakers: &[u32],
    speaker_actor: Vec<u8>,
    template_actor: Option<Vec<u8>>,
) {
    fs::create_dir_all(path).unwrap();
    let relation = field(
        b"XNAM",
        &[
            0x301_u32.to_le_bytes().as_slice(),
            (-5_i32).to_le_bytes().as_slice(),
            u32::MAX.to_le_bytes().as_slice(),
        ]
        .concat(),
    );
    let mut speaker_faction = field(b"DATA", &[1, 0, 0, 0]);
    speaker_faction.extend(relation);
    let target_faction = field(b"DATA", &[1, 0, 0, 0]);
    let mut records = vec![
        header(&[]),
        disk(b"NPC_", 0x100, 0, &speaker_actor),
        disk(b"NPC_", 0x101, 0, &actor(0x301, -3)),
        disk(b"FACT", 0x300, 0, &speaker_faction),
        disk(b"FACT", 0x301, 0, &target_faction),
    ];
    if let Some(template_actor) = template_actor {
        records.push(disk(b"NPC_", 0x102, 0, &template_actor));
    }
    records.extend([
        disk(b"INFO", 0x700, 0, &info(speakers)),
        disk(b"ACHR", 0x600, 0, &placed(0x100, 10)),
        disk(b"ACHR", 0x601, 0, &placed(0x101, 11)),
    ]);
    fs::write(path.join("FalloutNV.esm"), records.concat()).unwrap();
}

fn with_sources(
    path: &Path,
    speakers: &[u32],
    callback: impl FnOnce(
        &mut RecordStore,
        &Content,
        Sources<'_>,
        &mut World<'_>,
        ReferenceId,
        ReferenceId,
    ),
) {
    fixture(path, speakers);
    let mut store =
        RecordStore::open_nv_headers(path, &["FalloutNV.esm".into()], plugin::Limits::default())
            .unwrap();
    let scripts =
        loaded_scripts::Catalogue::load(&mut store, Default::default(), |_, _| Ok(())).unwrap();
    let content = Content::load(&mut store, &scripts, 1024).unwrap();
    let inventory = inventory::Catalogue::load(&mut store, Default::default()).unwrap();
    let actors = actors::Catalogue::load(&inventory, Default::default()).unwrap();
    let placements = placements::Catalogue::load(&mut store, Default::default()).unwrap();
    let associations =
        associations::Catalogue::load(&mut store, &actors, Default::default()).unwrap();
    let dependencies = {
        let lists = leveled::Catalogue::load(&mut store, Default::default()).unwrap();
        dependencies::Catalogue::load(
            &mut store,
            &actors,
            &associations,
            &lists,
            Default::default(),
        )
        .unwrap()
    };
    let factions = factions::Catalogue::load(&mut store, Default::default()).unwrap();
    let sources = Sources {
        placements: &placements,
        actors: &actors,
        associations: &associations,
        dependencies: &dependencies,
        factions: &factions,
    };
    let mut world = World::new(&scripts, WorldLimits::default()).unwrap();
    let speaker = world.register_reference(Some(form(0x600))).unwrap();
    let target = world.register_reference(Some(form(0x601))).unwrap();
    callback(&mut store, &content, sources, &mut world, speaker, target);
}

#[test]
fn unique_info_speaker_joins_same_current_context_for_stats_and_directed_relationships() {
    let temp = tempfile::tempdir().unwrap();
    with_sources(
        temp.path(),
        &[0x100],
        |store, content, sources, world, speaker, target| {
            let before = world.snapshot();
            let requests = dialogue_context::Requests::prepare(
                store,
                world,
                content,
                sources,
                Choice {
                    info: form(0x700),
                    speaker_reference: speaker,
                    target_reference: target,
                },
                Default::default(),
            )
            .unwrap();
            let observed = requests
                .observe(store, world, content, Default::default())
                .unwrap();

            assert_eq!(observed.info.key, form(0x700));
            assert_eq!(observed.speaker.actor_key, form(0x100));
            assert_eq!(observed.speaker_context.actor.key, &form(0x100));
            assert_eq!(observed.statistics.actor_key, &form(0x100));
            assert!(observed.statistics.current_actor_values.is_none());
            assert_eq!(observed.relationships.from.context.actor.key, &form(0x100));
            assert_eq!(observed.relationships.to.context.actor.key, &form(0x101));
            assert_eq!(observed.relationships.relations.len(), 1);
            assert!(observed.speaker_reference_matches_info);
            assert!(!observed.response_selection_supported);
            assert!(!observed.dialogue_conditions_supported);
            assert!(!observed.effective_faction_membership_supported);
            assert!(!observed.relationship_evaluation_supported);
            assert!(!observed.dialogue_eligibility_supported);
            assert_eq!(world.snapshot(), before);

            assert!(matches!(
                dialogue_context::Requests::prepare(
                    store,
                    world,
                    content,
                    sources,
                    Choice {
                        info: form(0x700),
                        speaker_reference: target,
                        target_reference: speaker,
                    },
                    Default::default(),
                ),
                Err(dialogue_context::Error::SpeakerReferenceMismatch)
            ));
            assert_eq!(world.snapshot(), before);
        },
    );
}

#[test]
fn repeated_physical_info_speakers_are_ambiguous_even_when_one_matches() {
    let temp = tempfile::tempdir().unwrap();
    with_sources(
        temp.path(),
        &[0x100, 0x101],
        |store, content, sources, world, speaker, target| {
            assert!(matches!(
                dialogue_context::Requests::prepare(
                    store,
                    world,
                    content,
                    sources,
                    Choice {
                        info: form(0x700),
                        speaker_reference: speaker,
                        target_reference: target,
                    },
                    Default::default(),
                ),
                Err(dialogue_context::Error::AmbiguousSpeaker)
            ));
        },
    );
}

#[test]
fn template_stat_candidates_stay_bound_to_speaker_and_faction_inheritance_is_explicitly_unsupported()
 {
    let temp = tempfile::tempdir().unwrap();
    fixture_with_speaker(
        temp.path(),
        &[0x100],
        actor_with_template(0, 0, 0x06, Some(0x102)),
        Some(actor(0x300, 2)),
    );
    with_sources(
        temp.path(),
        &[0x100],
        |store, content, sources, world, speaker, target| {
            let before = world.snapshot();
            let requests = dialogue_context::Requests::prepare(
                store,
                world,
                content,
                sources,
                Choice {
                    info: form(0x700),
                    speaker_reference: speaker,
                    target_reference: target,
                },
                Default::default(),
            )
            .unwrap();
            let observed = requests
                .observe(store, world, content, Default::default())
                .unwrap();

            assert_eq!(observed.speaker_context.actor.key, &form(0x100));
            assert_eq!(observed.statistics.actor_key, &form(0x100));
            assert!(
                observed
                    .statistics
                    .candidate_scalars
                    .iter()
                    .any(|candidate| candidate.definition.key == &form(0x102))
            );
            assert!(
                observed
                    .relationships
                    .from
                    .issues
                    .iter()
                    .any(|issue| issue.code == "faction_template_selection_unsupported")
            );
            assert!(observed.relationships.relations.is_empty());
            assert!(!observed.dialogue_eligibility_supported);
            assert_eq!(world.snapshot(), before);
        },
    );
}
