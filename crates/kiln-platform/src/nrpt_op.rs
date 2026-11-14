//! Names without the hosts file ([ADR 0078]): one rule in Windows' Name Resolution Policy
//! Table sends every name under Kiln's TLD to kilnd's resolver on 127.0.0.1. Previewed,
//! journaled, verified and undone like the hosts block; added and removed by Kiln's helper,
//! which only ever touches the rule carrying Kiln's marker.
//!
//! [ADR 0078]: ../../../docs/adr/0078-names-without-the-hosts-file.md

use std::rc::Rc;

use kiln_core::Tld;
use kiln_ops::{Change, OpError, Operation, Preview, Receipt, Registry, Scope};
use serde_json::{Value, json};

pub const NRPT_KIND: &str = "nrpt_rule";

/// The comment on Kiln's rule: the helper adds and removes only a rule with it.
pub const MARKER: &str = "Kiln (ADR 0078)";

/// What Windows has for a namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleState {
    /// Kiln's rule (its marker).
    Ours,
    /// Another program's rule: Kiln leaves it, and stays on the hosts file.
    Foreign,
    /// No rule.
    None,
}

/// Adds or removes Kiln's rule for a TLD: the helper, elevated.
pub trait PrivilegedNrpt {
    fn set_rule(&self, tld: &Tld, on: bool) -> Result<(), OpError>;
}

/// Reads what Windows has for a TLD (no elevation needed).
pub trait NrptReader {
    fn state(&self, tld: &Tld) -> Result<RuleState, OpError>;
}

/// Kiln's rule for `tld` on (`on`) or off.
pub struct NrptRuleOp {
    tld: Tld,
    on: bool,
    writer: Rc<dyn PrivilegedNrpt>,
    reader: Rc<dyn NrptReader>,
}

impl NrptRuleOp {
    #[must_use]
    pub fn new(
        tld: Tld,
        on: bool,
        writer: Rc<dyn PrivilegedNrpt>,
        reader: Rc<dyn NrptReader>,
    ) -> Self {
        Self {
            tld,
            on,
            writer,
            reader,
        }
    }

    /// Rebuilt from the journal (undo, crash recovery).
    pub fn from_params(
        writer: Rc<dyn PrivilegedNrpt>,
        reader: Rc<dyn NrptReader>,
        params: &Value,
    ) -> Result<Self, OpError> {
        let tld = params["tld"]
            .as_str()
            .and_then(|t| Tld::recorded(t).ok())
            .ok_or_else(|| OpError::Failed("params.tld is missing or not a TLD".into()))?;
        let on = params["on"]
            .as_bool()
            .ok_or_else(|| OpError::Failed("params.on is missing".into()))?;
        Ok(Self::new(tld, on, writer, reader))
    }

    fn wanted(&self) -> RuleState {
        if self.on {
            RuleState::Ours
        } else {
            RuleState::None
        }
    }

    fn describe(state: RuleState, tld: &Tld) -> String {
        match state {
            RuleState::Ours => format!(".{tld} → 127.0.0.1 (Kiln's rule)"),
            RuleState::Foreign => format!(".{tld} → another program's rule"),
            RuleState::None => "(no rule)".to_owned(),
        }
    }
}

impl std::fmt::Debug for NrptRuleOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NrptRuleOp")
            .field("tld", &self.tld.as_str())
            .field("on", &self.on)
            .finish_non_exhaustive()
    }
}

impl Operation for NrptRuleOp {
    fn kind(&self) -> &'static str {
        NRPT_KIND
    }

    fn params(&self) -> Value {
        json!({ "tld": self.tld.as_str(), "on": self.on })
    }

    fn scope(&self) -> Scope {
        Scope::Machine
    }

    fn preview(&self) -> Result<Preview, OpError> {
        let current = self.reader.state(&self.tld)?;
        let summary = if self.on {
            format!(
                "Send every name under .{} to Kiln on this PC (127.0.0.1): one rule in Windows' name resolution policy",
                self.tld
            )
        } else {
            format!(
                "Remove Kiln's .{} rule from Windows' name resolution policy",
                self.tld
            )
        };
        Ok(Preview {
            summary,
            changes: vec![Change {
                target: format!("Windows name resolution policy, .{}", self.tld),
                before: Some(Self::describe(current, &self.tld)),
                after: Some(Self::describe(self.wanted(), &self.tld)),
            }],
        })
    }

    fn prepare(&self) -> Result<Receipt, OpError> {
        let current = self.reader.state(&self.tld)?;
        if current == RuleState::Foreign {
            return Err(OpError::Failed(format!(
                "another program has a name resolution rule for .{}: Kiln leaves it, and stays on the hosts file",
                self.tld
            )));
        }
        Ok(Receipt(json!({ "previous": current == RuleState::Ours })))
    }

    fn apply(&self, _receipt: &Receipt) -> Result<(), OpError> {
        if self.reader.state(&self.tld)? == self.wanted() {
            return Ok(()); // already so: no elevation needed
        }
        self.writer.set_rule(&self.tld, self.on)
    }

    fn verify(&self) -> Result<(), OpError> {
        let current = self.reader.state(&self.tld)?;
        if current == self.wanted() {
            Ok(())
        } else {
            Err(OpError::VerifyFailed(format!(
                "Windows has {} for .{}",
                Self::describe(current, &self.tld),
                self.tld
            )))
        }
    }

    fn revert(&self, receipt: &Receipt, _force: bool) -> Result<(), OpError> {
        let previous = receipt.0["previous"].as_bool().unwrap_or(false);
        let current = self.reader.state(&self.tld)?;
        let back = if previous {
            RuleState::Ours
        } else {
            RuleState::None
        };
        if current == back || current == RuleState::Foreign {
            return Ok(());
        }
        self.writer.set_rule(&self.tld, previous)
    }
}

/// Lets the engine rebuild [`NrptRuleOp`]s from the journal (undo, crash recovery).
pub fn register_nrpt_rule(
    registry: &mut Registry,
    writer: Rc<dyn PrivilegedNrpt>,
    reader: Rc<dyn NrptReader>,
) {
    registry.register(NRPT_KIND, move |params| {
        Ok(Box::new(NrptRuleOp::from_params(
            writer.clone(),
            reader.clone(),
            params,
        )?))
    });
}

/// Windows' own rules, read without elevation through `Get-DnsClientNrptRule`.
#[cfg(windows)]
#[derive(Debug)]
pub struct SystemNrpt;

#[cfg(windows)]
impl NrptReader for SystemNrpt {
    fn state(&self, tld: &Tld) -> Result<RuleState, OpError> {
        let script = format!(
            "Get-DnsClientNrptRule | Where-Object {{ $_.Namespace -contains '.{tld}' }} | ForEach-Object {{ if ($_.Comment -eq '{MARKER}') {{ 'ours' }} else {{ 'foreign' }} }}"
        );
        let out = powershell(&script)?;
        Ok(state_of(&out))
    }
}

/// The state from the reader's lines: any foreign rule wins over ours.
#[must_use]
pub fn state_of(lines: &str) -> RuleState {
    let words: Vec<&str> = lines
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if words.contains(&"foreign") {
        RuleState::Foreign
    } else if words.contains(&"ours") {
        RuleState::Ours
    } else {
        RuleState::None
    }
}

/// Runs Windows PowerShell by its full System32 path, without a profile or prompts or a
/// window; its standard output.
#[cfg(windows)]
pub fn powershell(script: &str) -> Result<String, OpError> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
    let exe = std::path::Path::new(&root)
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe");
    let output = std::process::Command::new(exe)
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| OpError::Failed(format!("cannot run PowerShell: {e}")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(OpError::Failed(format!(
            "PowerShell failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// Windows as a cell: a rule or none, and the helper's calls.
    #[derive(Default)]
    struct Fake {
        state: Cell<Option<RuleState>>,
        calls: RefCell<Vec<bool>>,
    }
    impl NrptReader for Fake {
        fn state(&self, _tld: &Tld) -> Result<RuleState, OpError> {
            Ok(self.state.get().unwrap_or(RuleState::None))
        }
    }
    impl PrivilegedNrpt for Fake {
        fn set_rule(&self, _tld: &Tld, on: bool) -> Result<(), OpError> {
            self.calls.borrow_mut().push(on);
            self.state
                .set(Some(if on { RuleState::Ours } else { RuleState::None }));
            Ok(())
        }
    }

    fn op(fake: &Rc<Fake>, on: bool) -> NrptRuleOp {
        NrptRuleOp::new(Tld::default(), on, fake.clone(), fake.clone())
    }

    #[test]
    fn on_adds_kilns_rule_and_undo_removes_it() {
        let fake = Rc::new(Fake::default());
        let on = op(&fake, true);
        let receipt = on.prepare().unwrap();
        on.apply(&receipt).unwrap();
        on.verify().unwrap();
        assert_eq!(*fake.calls.borrow(), [true]);
        on.revert(&receipt, false).unwrap();
        assert_eq!(fake.state.get(), Some(RuleState::None));
        assert_eq!(*fake.calls.borrow(), [true, false]);
    }

    #[test]
    fn another_programs_rule_is_left_alone() {
        let fake = Rc::new(Fake::default());
        fake.state.set(Some(RuleState::Foreign));
        let err = op(&fake, true).prepare().unwrap_err();
        assert!(err.to_string().contains("another program"), "{err}");
        assert!(fake.calls.borrow().is_empty());
        // Undoing never removes someone else's rule either.
        op(&fake, false)
            .revert(&Receipt(json!({"previous": false})), false)
            .unwrap();
        assert!(fake.calls.borrow().is_empty());
    }

    #[test]
    fn rebuilds_from_its_params_and_reads_the_lines() {
        let fake = Rc::new(Fake::default());
        let rebuilt =
            NrptRuleOp::from_params(fake.clone(), fake.clone(), &op(&fake, true).params()).unwrap();
        assert_eq!(rebuilt.params(), json!({"tld": "test", "on": true}));
        assert_eq!(state_of("ours\r\n"), RuleState::Ours);
        assert_eq!(state_of("ours\nforeign\n"), RuleState::Foreign);
        assert_eq!(state_of(""), RuleState::None);
    }
}
