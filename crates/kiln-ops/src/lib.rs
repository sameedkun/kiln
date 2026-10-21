//! The operation engine: every change Kiln makes outside its own data directory goes through
//! here so it can be previewed, journaled, verified and reverted.
//!
//! Lifecycle of one operation inside [`Engine::apply`]:
//!
//! 1. `prepare` captures the prior state into a [`Receipt`] without changing anything.
//! 2. The receipt is journaled as [`OpState::Prepared`] *before* the system is touched, so a
//!    crash at any later point can be rolled back by [`Engine::recover`].
//! 3. `apply` makes the change and `verify` checks it took effect.
//! 4. The entry is journaled as [`OpState::Applied`].
//!
//! If any step fails, the failing operation and every operation already applied in the same
//! batch are reverted in reverse order, so a batch is all-or-nothing.

mod engine;
mod journal;
mod operation;
#[cfg(any(test, feature = "testing"))]
pub mod testing;

pub use engine::{ApplyError, BatchReport, Engine, RecoveryReport, Registry};
pub use journal::{FileJournal, Journal, JournalEntry, JournalError, MemoryJournal, OpState};
pub use operation::{Change, OpError, Operation, Preview, Receipt, Scope};
