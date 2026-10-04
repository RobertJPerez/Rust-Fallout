//! One explicitly selected canonical item lot, without equip/mod selection rules.
use crate::{
    World,
    foreign::{Content, SourceForm},
    identity::ReferenceId,
    inventory::{InventoryView, Item, ItemHandle, ItemId, ViewLimits},
};
use fallout_data::identity::FormKey;
use serde::Serialize;
use std::io::Write;

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub inventory: ViewLimits,
    pub max_visits: usize,
    pub max_projection_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            inventory: ViewLimits {
                max_items: 16_384,
                max_links: 100_000,
                max_extra_bytes: 16 * 1024 * 1024,
            },
            max_visits: 100_000,
            max_projection_bytes: 32 * 1024 * 1024,
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("equipment item request inventory is uninitialized")]
    InventoryUninitialized,
    #[error("equipment item request inventory is explicitly empty")]
    InventoryEmpty,
    #[error("equipment selected item belongs to a different owner")]
    WrongOwner,
    #[error("equipment selected item is missing from this owner inventory")]
    ItemUnavailable,
    #[error("equipment item request {0} budget exceeded")]
    Capacity(&'static str),
    #[error(transparent)]
    State(#[from] crate::Error),
    #[error(transparent)]
    Content(#[from] crate::foreign::Failure),
}

/// Owned canonical observation with one private exact index into its ordered lots.
/// It cannot be deserialized into authority or used to equip/change an item.
#[derive(Debug, Serialize)]
pub struct Selection {
    inventory: InventoryView,
    selected_item_index: usize,
    source_form: SourceForm,
    visits: usize,
    equip_rules_supported: bool,
    active_mod_mask: Option<u8>,
    scope: &'static str,
}
impl Selection {
    pub fn inventory(&self) -> &InventoryView {
        &self.inventory
    }
    pub fn selected_lot(&self) -> &Item {
        &self.inventory.items().expect("selected initialized bank")[self.selected_item_index]
    }
    pub fn base(&self) -> &FormKey {
        &self.selected_lot().facts().base
    }
    pub fn source_form(&self) -> SourceForm {
        self.source_form
    }
    pub fn visits(&self) -> usize {
        self.visits
    }
}
struct ProjectionBudget {
    bytes: usize,
    maximum: usize,
}
impl Write for ProjectionBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .filter(|n| *n <= self.maximum)
            .ok_or_else(|| {
                std::io::Error::other("equipment item request projection byte budget")
            })?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// ItemId is explicitly interpreted in the supplied current World campaign.
/// UI handles should use observe_handle so a restored/foreign epoch refuses.
pub fn observe(
    world: &World<'_>,
    content: &Content,
    owner: ReferenceId,
    item: ItemId,
    limits: Limits,
) -> Result<Selection, Error> {
    content.validate_world(world)?;
    let inventory = world.inventory_view(owner, limits.inventory)?;
    let items = inventory.items().ok_or(Error::InventoryUninitialized)?;
    if items.is_empty() {
        return Err(Error::InventoryEmpty);
    }
    let visits = items.len().checked_add(2).ok_or(Error::Capacity("visit"))?;
    if visits > limits.max_visits {
        return Err(Error::Capacity("visit"));
    }
    let selected_item_index = match items.binary_search_by_key(&item, Item::id) {
        Ok(index) => index,
        Err(_) => {
            if world.item(item).is_ok_and(|lot| lot.owner() != owner) {
                return Err(Error::WrongOwner);
            }
            return Err(Error::ItemUnavailable);
        }
    };
    let selected = &items[selected_item_index];
    if selected.owner() != owner {
        return Err(Error::WrongOwner);
    }
    let source_form = content.source_form(world, &selected.facts().base)?;
    let result = Selection {
        inventory,
        selected_item_index,
        source_form,
        visits,
        equip_rules_supported: false,
        active_mod_mask: None,
        scope: "One explicit canonical owner/item lot and exact optional facts with winning source header; inventory presence/slot/modification metadata never chooses equipped state, active weapon mod mask, model role, attachment or retail item rules",
    };
    serde_json::to_writer(
        ProjectionBudget {
            bytes: 0,
            maximum: limits.max_projection_bytes,
        },
        &result,
    )
    .map_err(|_| Error::Capacity("projection byte"))?;
    Ok(result)
}

pub fn observe_handle(
    world: &World<'_>,
    content: &Content,
    owner: ReferenceId,
    item: ItemHandle,
    limits: Limits,
) -> Result<Selection, Error> {
    let id = world.item_id(item)?;
    observe(world, content, owner, id, limits)
}
