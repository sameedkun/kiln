//! Kiln's durable state, stored in one SQLite database per user.
//!
//! Phase 1 starts with the operation journal ([`StateStore`] implements
//! [`kiln_ops::Journal`]) and a small settings table. Sites, runtimes and services join the
//! schema through numbered migrations as they are built.
//!
//! Durability: WAL mode with `synchronous = FULL`, so a recorded journal entry survives power
//! loss, matching the fsync-per-event guarantee of the JSON Lines journal it replaces.

mod import;
mod paths;
mod schema;
mod store;

pub use import::ImportReport;
pub use paths::{DB_FILE, LEGACY_JOURNAL_FILE, default_state_dir};
pub use schema::SCHEMA_VERSION;
pub use store::{OpenReport, StateError, StateStore};
