//! Source-bound capability checks. Engineering observations are not VM effects.
pub mod admission;
pub mod attachment_boot;
pub mod condition;
pub mod copy_probe;
pub mod event_request;
pub mod fixture;
pub mod foreign_copy;
pub mod literal_assignment;
pub mod local_copy;
pub mod native;
pub mod native_plan;
pub mod pending_batch;
pub mod reference_attachment_boot;
pub mod reference_copy;
pub mod trace;
