use std::fmt;

use serde::{Deserialize, Serialize};

/// Who is affected by an operation, which also decides whether it needs elevation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    User,
    Machine,
}

/// One concrete change shown to the user before anything is applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Change {
    /// What is changed, e.g. a file path, registry value or certificate store.
    pub target: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

/// Human-readable description of an operation, rendered by the CLI and the GUI's
/// confirmation sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    pub summary: String,
    pub changes: Vec<Change>,
}

/// The prior state an operation needs to undo itself. Stored in the journal verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Receipt(pub serde_json::Value);

#[derive(Debug, thiserror::Error)]
pub enum OpError {
    #[error("{0}")]
    Failed(String),
    #[error("verification failed: {0}")]
    VerifyFailed(String),
    #[error("target changed since it was applied ({0}); rerun with force to overwrite")]
    Drifted(String),
    #[error("no operation kind `{0}` is registered")]
    UnknownKind(String),
    #[error("invalid parameters for `{kind}`: {source}")]
    BadParams {
        kind: String,
        source: serde_json::Error,
    },
}

/// A reversible change to the system.
///
/// Implementations must be reconstructible from `kind()` + `params()` through a
/// [`crate::Registry`], because undo can happen in a later process than apply.
pub trait Operation: fmt::Debug {
    /// Stable identifier such as `dns.hosts.block`, stored in the journal.
    fn kind(&self) -> &'static str;

    /// Parameters needed to rebuild this operation for a later revert.
    fn params(&self) -> serde_json::Value;

    fn scope(&self) -> Scope;

    fn requires_elevation(&self) -> bool {
        self.scope() == Scope::Machine
    }

    /// Describes the change against the current system state. Must not modify anything.
    fn preview(&self) -> Result<Preview, OpError>;

    /// Captures everything needed to undo the change. Must not modify anything.
    fn prepare(&self) -> Result<Receipt, OpError>;

    fn apply(&self, receipt: &Receipt) -> Result<(), OpError>;

    /// Checks that the system is in the state `apply` intended. Must not modify anything.
    fn verify(&self) -> Result<(), OpError>;

    /// Restores the state recorded in `receipt`.
    ///
    /// Must be idempotent: reverting an operation that never got applied, or was already
    /// reverted, succeeds. If the target changed after apply in a way the operation cannot
    /// merge, it returns [`OpError::Drifted`] unless `force` is set.
    fn revert(&self, receipt: &Receipt, force: bool) -> Result<(), OpError>;
}
