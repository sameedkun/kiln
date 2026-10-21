//! Kiln's php-cgi supervisor ([ADR 0014]):
//!
//! - [`pools`]: `pools.json`, the desired workers, with staggered recycle limits.
//! - [`reconcile`]: keeping running workers in line with it (respawn, crash back-off).
//! - [`launcher`]: starting real php-cgi workers inside a kill-on-close job.
//! - [`run`]: the `kiln-supervisor` main loop.
//! - [`handle`]: starting, updating and stopping it from the CLI.
//!
//! [ADR 0014]: ../../../docs/adr/0014-php-cgi-supervisor.md

pub mod handle;
pub mod launcher;
pub mod pools;
pub mod reconcile;
pub mod run;

pub use handle::{SupervisorError, SupervisorHandle};
pub use pools::{PoolSpec, Pools, WorkerSpec};
