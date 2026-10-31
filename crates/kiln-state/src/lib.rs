//! Kiln's durable state, stored in one SQLite database per user.
//!
//! It holds the operation journal ([`StateStore`] implements
//! [`kiln_ops::Journal`]), a settings table, linked sites, installed PHP runtimes and service
//! instances. New tables join the schema through numbered migrations as they are built.
//!
//! Durability: WAL mode with `synchronous = FULL`, so a recorded journal entry survives power
//! loss, matching the fsync-per-event guarantee of the JSON Lines journal it replaces.

mod import;
mod paths;
mod php;
mod proxies;
mod schema;
mod services;
mod sites;
mod store;

pub use import::ImportReport;
pub use paths::{DB_FILE, LEGACY_JOURNAL_FILE, default_local_dir, default_state_dir};
pub use php::PhpRecord;
pub use proxies::ProxyRecord;
pub use schema::SCHEMA_VERSION;
pub use services::ServiceRecord;
pub use sites::SiteRecord;
pub use store::{OpenReport, StateError, StateStore};
