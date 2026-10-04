//! Collection admission without owned state before the strict DTO decoder.
//! This is a counting pass, not a second snapshot schema validator. Serde still
//! rejects missing/duplicate/unknown fields and invalid values on the second
//! pass. JSON's bounded string scratch space may be used for escaped strings, but
//! this pass never constructs owned keys, collections or state objects.
use crate::{Error, Limits, Result};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use std::fmt;

#[derive(Clone, Copy)]
pub(super) enum Schema {
    Current,
    V1,
    V2,
    V3,
}

#[derive(Clone, Copy)]
enum Shape {
    Snapshot(Schema),
    Banks,
    Bank,
    Items,
    Item,
    Facts,
    Links,
    Extras,
    Extra,
    Bytes,
    OptionalLink,
    References,
    ReferenceStates,
    Instances,
    Instance,
    Locals,
    Events,
    Event,
    Context(bool),
    Arguments(bool),
    Other,
}

#[derive(Clone, Copy)]
enum Budget {
    Banks,
    Items,
    Links,
    Bytes,
    References,
    ReferenceStates,
    Instances,
    Locals,
    Events,
    Arguments(bool),
}

impl Shape {
    fn collection(self) -> Option<(Self, Budget)> {
        Some(match self {
            Self::Banks => (Self::Bank, Budget::Banks),
            Self::Items => (Self::Item, Budget::Items),
            Self::Links => (Self::Other, Budget::Links),
            Self::Extras => (Self::Extra, Budget::Links),
            Self::Bytes => (Self::Other, Budget::Bytes),
            Self::References => (Self::Other, Budget::References),
            Self::ReferenceStates => (Self::Other, Budget::ReferenceStates),
            Self::Instances => (Self::Instance, Budget::Instances),
            Self::Locals => (Self::Other, Budget::Locals),
            Self::Events => (Self::Event, Budget::Events),
            Self::Arguments(event) => (Self::Other, Budget::Arguments(event)),
            _ => return None,
        })
    }

    fn field(self, name: &str) -> Self {
        match (self, name) {
            (Self::Snapshot(Schema::Current | Schema::V3), "inventory_banks") => Self::Banks,
            (Self::Snapshot(Schema::Current), "reference_states") => Self::ReferenceStates,
            (Self::Snapshot(_), "references") => Self::References,
            (Self::Snapshot(_), "instances") => Self::Instances,
            (Self::Snapshot(_), "pending_events") => Self::Events,
            (Self::Bank, "items") => Self::Items,
            (Self::Item, "facts") => Self::Facts,
            (Self::Facts, "equipped_slots" | "modifications") => Self::Links,
            (Self::Facts, "extra_fields") => Self::Extras,
            (Self::Facts, "ownership" | "ammo" | "script_instance") => Self::OptionalLink,
            (Self::Extra, "bytes") => Self::Bytes,
            (Self::Instance, "context") => Self::Context(false),
            (Self::Instance, "locals") => Self::Locals,
            (Self::Event, "context") => Self::Context(true),
            (Self::Context(event), "arguments") => Self::Arguments(event),
            _ => Self::Other,
        }
    }

    // Serde structs also accept positional arrays. These positions follow the
    // existing DTO field order, including each legacy schema's distinct order.
    fn position(self, index: usize) -> Self {
        match (self, index) {
            (Self::Snapshot(Schema::Current | Schema::V3), 6) => Self::Banks,
            (Self::Snapshot(Schema::Current), 14) => Self::ReferenceStates,
            (Self::Snapshot(Schema::Current | Schema::V3), 11)
            | (Self::Snapshot(Schema::V1), 7)
            | (Self::Snapshot(Schema::V2), 9) => Self::References,
            (Self::Snapshot(Schema::Current | Schema::V3), 12)
            | (Self::Snapshot(Schema::V1), 8)
            | (Self::Snapshot(Schema::V2), 10) => Self::Instances,
            (Self::Snapshot(Schema::Current | Schema::V3), 13)
            | (Self::Snapshot(Schema::V1), 9)
            | (Self::Snapshot(Schema::V2), 11) => Self::Events,
            (Self::Bank, 1) => Self::Items,
            (Self::Item, 3) => Self::Facts,
            (Self::Facts, 2 | 4 | 7) => Self::OptionalLink,
            (Self::Facts, 3 | 5) => Self::Links,
            (Self::Facts, 8) => Self::Extras,
            (Self::Extra, 1) => Self::Bytes,
            (Self::Instance, 3) => Self::Context(false),
            (Self::Instance, 4) => Self::Locals,
            (Self::Event, 3) => Self::Context(true),
            (Self::Context(event), 3) => Self::Arguments(event),
            _ => Self::Other,
        }
    }
}

#[derive(Default)]
struct Counts {
    banks: usize,
    items: usize,
    links: usize,
    bytes: usize,
    item_links: usize,
    item_bytes: usize,
    references: usize,
    reference_states: usize,
    instances: usize,
    locals: usize,
    events: usize,
}

struct Scan {
    limits: Limits,
    counts: Counts,
    capacity: Option<&'static str>,
}

fn increment(
    count: &mut usize,
    maximum: usize,
    label: &'static str,
) -> std::result::Result<(), &'static str> {
    if *count >= maximum {
        return Err(label);
    }
    *count += 1;
    Ok(())
}

impl Scan {
    fn take<E: de::Error>(
        &mut self,
        budget: Budget,
        in_collection: usize,
    ) -> std::result::Result<(), E> {
        let c = &mut self.counts;
        let l = self.limits;
        let result = match budget {
            Budget::Banks => {
                increment(&mut c.banks, l.max_inventory_banks, "saved inventory banks")
            }
            Budget::Items => increment(&mut c.items, l.max_item_instances, "saved items"),
            Budget::Links => increment(
                &mut c.item_links,
                l.max_item_links,
                "saved item extra state",
            )
            .and_then(|()| {
                increment(
                    &mut c.links,
                    l.max_total_item_links,
                    "saved item extra state",
                )
            }),
            Budget::Bytes => increment(
                &mut c.item_bytes,
                l.max_item_bytes,
                "saved item extra state",
            )
            .and_then(|()| {
                increment(
                    &mut c.bytes,
                    l.max_total_item_bytes,
                    "saved item extra state",
                )
            }),
            Budget::References => {
                increment(&mut c.references, l.max_references, "saved references")
            }
            Budget::ReferenceStates => increment(
                &mut c.reference_states,
                l.max_references,
                "saved reference states",
            ),
            Budget::Instances => increment(&mut c.instances, l.max_instances, "saved instances"),
            Budget::Locals => increment(&mut c.locals, l.max_locals, "saved locals"),
            Budget::Events => increment(&mut c.events, l.max_pending_events, "saved events"),
            Budget::Arguments(event) => {
                if in_collection >= l.max_event_arguments {
                    Err(if event {
                        "saved event context arguments"
                    } else {
                        "saved instance context arguments"
                    })
                } else {
                    Ok(())
                }
            }
        };
        result.map_err(|label| {
            self.capacity = Some(label);
            E::custom(label)
        })
    }
}

struct Node<'a> {
    scan: &'a mut Scan,
    shape: Shape,
    depth: usize,
    charge: Option<(Budget, usize)>,
}

impl<'de> DeserializeSeed<'de> for Node<'_> {
    type Value = ();
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        if let Some((budget, index)) = self.charge {
            self.scan.take::<D::Error>(budget, index)?;
        }
        if matches!(self.shape, Shape::Item) {
            self.scan.counts.item_links = 0;
            self.scan.counts.item_bytes = 0;
        }
        // Every nested value, including irrelevant/malformed fields, goes
        // through this seed. IgnoredAny can skip nested containers without
        // applying serde_json's recursion guard, so it is never used here.
        if self.depth > 128 {
            return Err(de::Error::custom("snapshot nesting limit exceeded"));
        }
        deserializer.deserialize_any(self)
    }
}

impl Node<'_> {
    fn present<E: de::Error>(&mut self) -> std::result::Result<(), E> {
        if matches!(self.shape, Shape::OptionalLink) {
            self.scan.take::<E>(Budget::Links, 0)?;
        }
        Ok(())
    }
}

impl<'de> Visitor<'de> for Node<'_> {
    type Value = ();
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded snapshot JSON")
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_bool<E: de::Error>(mut self, _: bool) -> std::result::Result<(), E> {
        self.present()
    }
    fn visit_i64<E: de::Error>(mut self, _: i64) -> std::result::Result<(), E> {
        self.present()
    }
    fn visit_u64<E: de::Error>(mut self, _: u64) -> std::result::Result<(), E> {
        self.present()
    }
    fn visit_f64<E: de::Error>(mut self, _: f64) -> std::result::Result<(), E> {
        self.present()
    }
    fn visit_str<E: de::Error>(mut self, _: &str) -> std::result::Result<(), E> {
        self.present()
    }
    fn visit_map<A: MapAccess<'de>>(mut self, mut map: A) -> std::result::Result<(), A::Error> {
        self.present()?;
        while let Some(shape) = map.next_key_seed(Key(self.shape))? {
            map.next_value_seed(Node {
                scan: self.scan,
                shape,
                depth: self.depth + 1,
                charge: None,
            })?;
        }
        Ok(())
    }
    fn visit_seq<A: SeqAccess<'de>>(mut self, mut seq: A) -> std::result::Result<(), A::Error> {
        self.present()?;
        let collection = self.shape.collection();
        let mut index = 0;
        loop {
            let (shape, charge) = match collection {
                Some((shape, budget)) => (shape, Some((budget, index))),
                None => (self.shape.position(index), None),
            };
            if seq
                .next_element_seed(Node {
                    scan: self.scan,
                    shape,
                    depth: self.depth + 1,
                    charge,
                })?
                .is_none()
            {
                break;
            }
            index += 1;
        }
        Ok(())
    }
}

struct Key(Shape);
impl<'de> DeserializeSeed<'de> for Key {
    type Value = Shape;
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<Shape, D::Error> {
        deserializer.deserialize_identifier(self)
    }
}
impl<'de> Visitor<'de> for Key {
    type Value = Shape;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("snapshot field name")
    }
    fn visit_str<E: de::Error>(self, name: &str) -> std::result::Result<Shape, E> {
        Ok(self.0.field(name))
    }
}

pub(super) fn check(bytes: &[u8], limits: Limits, schema: Schema) -> Result<()> {
    let mut scan = Scan {
        limits,
        counts: Counts::default(),
        capacity: None,
    };
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let result = Node {
        scan: &mut scan,
        shape: Shape::Snapshot(schema),
        depth: 0,
        charge: None,
    }
    .deserialize(&mut deserializer)
    .and_then(|()| deserializer.end());
    result.map_err(|error| {
        scan.capacity
            .map_or_else(|| Error::Json(error), Error::Capacity)
    })
}
