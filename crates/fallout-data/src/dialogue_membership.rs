//! Winning INFO membership from original topic-child GRUP labels. This is a
//! structural index, not the order in which the original engine selects dialogue.
use crate::{Error, Result, identity::FormKey, plugin, store::RecordStore};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Linked,
    DeletedInfo,
    MissingParent,
    NullParent,
    MissingTopic,
    DeletedTopic,
    WrongTopicKind,
}

#[derive(Debug, Serialize)]
pub struct Target {
    pub source_plugin: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub record_kind: String,
}

#[derive(Debug, Serialize)]
pub struct Row {
    pub info_key: FormKey,
    pub source_plugin: String,
    pub record_file_offset: u64,
    pub record_flags: u32,
    pub raw_parent_topic: Option<u32>,
    pub topic_key: Option<FormKey>,
    pub topic: Option<Target>,
    pub status: Status,
}

#[derive(Debug, Default, Serialize)]
pub struct Counts {
    pub winning_infos: u64,
    pub linked_infos: u64,
    pub deleted_infos: u64,
    pub missing_parents: u64,
    pub null_parents: u64,
    pub missing_topics: u64,
    pub deleted_topics: u64,
    pub wrong_topic_kinds: u64,
    pub topics_with_linked_infos: u64,
}

#[derive(Debug, Serialize)]
pub struct Topic {
    pub topic_key: FormKey,
    pub info_keys: Vec<FormKey>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub counts: Counts,
    pub rows: Vec<Row>,
    pub topics: Vec<Topic>,
    pub payloads_validated: bool,
    pub retail_selection_order_verified: bool,
}

pub struct MembershipIndex {
    report: Report,
    // Lists are canonical-key sorted for reproducible lookup. They are not a
    // retail response order, and selection must use a separately verified policy.
    by_topic: BTreeMap<FormKey, usize>,
}

impl MembershipIndex {
    pub fn report(&self) -> &Report {
        &self.report
    }
    pub fn into_report(self) -> Report {
        self.report
    }
    pub fn topic_infos(&self, topic: &FormKey) -> &[FormKey] {
        self.by_topic
            .get(topic)
            .map(|&index| self.report.topics[index].info_keys.as_slice())
            .unwrap_or(&[])
    }

    pub fn build(store: &RecordStore, max_infos: usize) -> Result<Self> {
        let mut counts = Counts::default();
        let mut rows = Vec::new();
        let mut topics = BTreeMap::<FormKey, Vec<FormKey>>::new();
        for (key, location) in store.winning_definitions() {
            let definition = store.definition(location);
            if definition.header.kind != *b"INFO" {
                continue;
            }
            if rows.len() >= max_infos {
                return Err(Error::Unsupported(
                    "dialogue membership budget exceeded".into(),
                ));
            }
            counts.winning_infos += 1;
            let raw = definition.parent.topic;
            let topic_key = raw
                .map(|raw| store.key_for(location, raw))
                .transpose()?
                .flatten();
            let target = topic_key.as_ref().and_then(|key| store.winner(key));
            let status = if definition.header.flags & plugin::DELETED != 0 {
                counts.deleted_infos += 1;
                Status::DeletedInfo
            } else if raw.is_none() {
                counts.missing_parents += 1;
                Status::MissingParent
            } else if topic_key.is_none() {
                counts.null_parents += 1;
                Status::NullParent
            } else if let Some(target) = target {
                let topic = store.definition(target);
                if topic.header.flags & plugin::DELETED != 0 {
                    counts.deleted_topics += 1;
                    Status::DeletedTopic
                } else if topic.header.kind != *b"DIAL" {
                    counts.wrong_topic_kinds += 1;
                    Status::WrongTopicKind
                } else {
                    counts.linked_infos += 1;
                    topics
                        .entry(topic_key.clone().expect("checked topic key"))
                        .or_default()
                        .push(key.clone());
                    Status::Linked
                }
            } else {
                counts.missing_topics += 1;
                Status::MissingTopic
            };
            rows.push(Row {
                info_key: key.clone(),
                source_plugin: store.source_name(location).into(),
                record_file_offset: definition.header.offset,
                record_flags: definition.header.flags,
                raw_parent_topic: raw,
                topic_key,
                topic: target.map(|location| {
                    let header = &store.definition(location).header;
                    Target {
                        source_plugin: store.source_name(location).into(),
                        record_file_offset: header.offset,
                        record_flags: header.flags,
                        record_kind: plugin::signature(header.kind),
                    }
                }),
                status,
            });
        }
        counts.topics_with_linked_infos = topics.len() as u64;
        let mut by_topic = BTreeMap::new();
        let mut topic_rows = Vec::with_capacity(topics.len());
        for (key, infos) in topics {
            by_topic.insert(key.clone(), topic_rows.len());
            topic_rows.push(Topic {
                topic_key: key,
                info_keys: infos,
            });
        }
        Ok(Self {
            report: Report {
                counts,
                rows,
                topics: topic_rows,
                payloads_validated: false,
                retail_selection_order_verified: false,
            },
            by_topic,
        })
    }
}
