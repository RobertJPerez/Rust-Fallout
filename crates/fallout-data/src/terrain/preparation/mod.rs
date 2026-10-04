//! Exact immutable exterior texture sources prepared through the existing job pool.
mod batch;
pub(in crate::terrain) mod budget;
mod plan;

pub use batch::{Ready, Receipt, TexturePreparation, TextureReceipt};
pub use budget::{Limits, Usage};
pub use plan::{PhysicalContext, PlanReceipt, TextureSourcePlan};
