//! Source-only model preparation for an immutable exact CELL request.
mod batch;
mod plan;

pub use batch::{CellPreparation, Ready, Receipt};
pub use plan::{ArchiveReceipt, CellModelPlan, Limits, ModelCoverage, PlanReceipt, RequestReceipt};
