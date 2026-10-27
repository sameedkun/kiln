//! Starting `kilnd` at login, reversibly ([ADR 0026] §6).
//!
//! - **Windows:** the value `Kiln` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`.
//! - **Linux:** an XDG autostart entry, `~/.config/autostart/kiln-kilnd.desktop`, started by
//!   the desktop session after login, when the keyring and D-Bus are available.
//!
//! Undo restores exactly what was there: no entry, or the one that was replaced. An entry
//! changed by hand since is left alone unless forced, as everywhere else.
//!
//! [ADR 0026]: ../../../docs/adr/0026-kilnd-daemon-and-local-api.md

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use kiln_ops::{Change, OpError, Operation, Preview, Receipt, Registry, Scope};
use serde_json::{Value, json};

/// Journal kind of [`AutostartOp`].
pub const AUTOSTART_KIND: &str = "daemon.autostart";

/// Where a login item is stored: the registry or a file, or memory in tests.
pub trait LoginItem: std::fmt::Debug {
    /// What `kiln history` and previews call it.
    fn location(&self) -> String;
    fn read(&self) -> Result<Option<String>, OpError>;
    fn write(&self, content: &str) -> Result<(), OpError>;
    fn remove(&self) -> Result<(), OpError>;
    /// Whether the folder or key holding it exists, so undo can remove one Kiln created.
    fn container_exists(&self) -> bool;
    /// Removes that folder if it is empty; does nothing if it isn't.
    fn remove_container(&self) -> Result<(), OpError>;
}

/// The login item's content for starting `kilnd` at `exe`.
#[must_use]
pub fn content_for(exe: &Path) -> String {
    if cfg!(windows) {
        format!("\"{}\"", exe.display())
    } else {
        desktop_entry(exe)
    }
}

/// An XDG autostart entry ([Desktop Entry spec]) for `exe`. `Exec` quotes the path, escaping
/// the characters the spec reserves inside quotes.
///
/// [Desktop Entry spec]: https://specifications.freedesktop.org/desktop-entry-spec/latest/exec-variables.html
#[must_use]
pub fn desktop_entry(exe: &Path) -> String {
    let mut quoted = String::from("\"");
    for c in exe.display().to_string().chars() {
        if matches!(c, '"' | '`' | '$' | '\\') {
            quoted.push('\\');
        }
        quoted.push(c);
    }
    quoted.push('"');
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Kiln daemon\n\
         Comment=Keeps Kiln's local web server running (kilnd)\n\
         Exec={quoted}\n\
         Terminal=false\n\
         NoDisplay=true\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

/// Makes `item` hold `content`, so the session starts `kilnd` at login.
#[derive(Debug, Clone)]
pub struct AutostartOp {
    content: String,
    item: Rc<dyn LoginItem>,
}

impl AutostartOp {
    #[must_use]
    pub fn new(content: String, item: Rc<dyn LoginItem>) -> Self {
        Self { content, item }
    }
}

impl Operation for AutostartOp {
    fn kind(&self) -> &'static str {
        AUTOSTART_KIND
    }

    fn params(&self) -> Value {
        json!({ "content": self.content, "location": self.item.location() })
    }

    fn scope(&self) -> Scope {
        Scope::User
    }

    fn preview(&self) -> Result<Preview, OpError> {
        let current = self.item.read()?;
        let already = current.as_deref() == Some(self.content.as_str());
        Ok(Preview {
            summary: if already {
                "Kiln's daemon already starts at login".into()
            } else {
                "Start Kiln's daemon (kilnd) when you log in".into()
            },
            changes: vec![Change {
                target: self.item.location(),
                before: current,
                after: Some(self.content.clone()),
            }],
        })
    }

    fn prepare(&self) -> Result<Receipt, OpError> {
        Ok(Receipt(json!({
            "before": self.item.read()?,
            "container_existed": self.item.container_exists(),
        })))
    }

    fn apply(&self, _receipt: &Receipt) -> Result<(), OpError> {
        if self.item.read()?.as_deref() == Some(self.content.as_str()) {
            return Ok(());
        }
        self.item.write(&self.content)
    }

    fn verify(&self) -> Result<(), OpError> {
        if self.item.read()?.as_deref() == Some(self.content.as_str()) {
            Ok(())
        } else {
            Err(OpError::VerifyFailed(format!(
                "{} doesn't start Kiln's daemon",
                self.item.location()
            )))
        }
    }

    fn revert(&self, receipt: &Receipt, force: bool) -> Result<(), OpError> {
        let before = receipt.0["before"].as_str();
        let current = self.item.read()?;
        if current.as_deref() == before {
            return Ok(()); // never applied, or already undone
        }
        if current.as_deref() != Some(self.content.as_str()) && !force {
            return Err(OpError::Drifted(format!(
                "{} was changed after Kiln set it; `--force` puts back what was there before",
                self.item.location()
            )));
        }
        if let Some(previous) = before {
            return self.item.write(previous);
        }
        self.item.remove()?;
        if !receipt.0["container_existed"].as_bool().unwrap_or(true) {
            self.item.remove_container()?;
        }
        Ok(())
    }
}

/// The `Run` value on Windows.
#[cfg(windows)]
#[derive(Debug)]
pub struct RunValue {
    pub name: String,
}

#[cfg(windows)]
impl Default for RunValue {
    fn default() -> Self {
        Self {
            name: "Kiln".into(),
        }
    }
}

#[cfg(windows)]
impl LoginItem for RunValue {
    fn location(&self) -> String {
        format!(r"HKCU\{}\{}", kiln_sys::win::USER_RUN_KEY, self.name)
    }

    fn read(&self) -> Result<Option<String>, OpError> {
        kiln_sys::win::read_user_reg_string(kiln_sys::win::USER_RUN_KEY, &self.name)
            .map(|v| v.map(|v| v.value))
            .map_err(|e| OpError::Failed(format!("cannot read {}: {e}", self.location())))
    }

    fn write(&self, content: &str) -> Result<(), OpError> {
        kiln_sys::win::write_user_reg_string(
            kiln_sys::win::USER_RUN_KEY,
            &self.name,
            &kiln_sys::win::RegString {
                value: content.to_owned(),
                expandable: false,
            },
        )
        .map_err(|e| OpError::Failed(format!("cannot write {}: {e}", self.location())))
    }

    fn remove(&self) -> Result<(), OpError> {
        kiln_sys::win::delete_user_reg_value(kiln_sys::win::USER_RUN_KEY, &self.name)
            .map_err(|e| OpError::Failed(format!("cannot remove {}: {e}", self.location())))
    }

    fn container_exists(&self) -> bool {
        true // the Run key is part of every user profile
    }

    fn remove_container(&self) -> Result<(), OpError> {
        Ok(())
    }
}

/// A file holding the login item (Linux's XDG autostart entry).
#[derive(Debug)]
pub struct FileItem {
    pub path: PathBuf,
}

impl FileItem {
    /// `$XDG_CONFIG_HOME/autostart/kiln-kilnd.desktop` (default `~/.config`).
    #[must_use]
    pub fn xdg_autostart() -> Option<Self> {
        let config = std::env::var_os("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(Self {
            path: config.join("autostart").join("kiln-kilnd.desktop"),
        })
    }
}

impl LoginItem for FileItem {
    fn location(&self) -> String {
        self.path.display().to_string()
    }

    fn read(&self) -> Result<Option<String>, OpError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(OpError::Failed(format!(
                "cannot read {}: {e}",
                self.location()
            ))),
        }
    }

    fn write(&self, content: &str) -> Result<(), OpError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| OpError::Failed(format!("cannot create {}: {e}", dir.display())))?;
        }
        // Replace an existing entry in one step; a new one is simply written.
        let written = if self.path.exists() {
            kiln_sys::write_replacing(&self.path, content.as_bytes())
        } else {
            std::fs::write(&self.path, content)
        };
        written.map_err(|e| OpError::Failed(format!("cannot write {}: {e}", self.location())))
    }

    fn remove(&self) -> Result<(), OpError> {
        match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(OpError::Failed(format!(
                "cannot remove {}: {e}",
                self.location()
            ))),
        }
    }

    fn container_exists(&self) -> bool {
        self.path.parent().is_some_and(Path::is_dir)
    }

    fn remove_container(&self) -> Result<(), OpError> {
        if let Some(dir) = self.path.parent() {
            // Fails if anything else is in it, which is exactly when it must stay.
            let _ = std::fs::remove_dir(dir);
        }
        Ok(())
    }
}

/// An in-memory login item for tests.
#[derive(Debug, Default)]
pub struct MemoryItem(pub RefCell<Option<String>>);

impl LoginItem for MemoryItem {
    fn location(&self) -> String {
        "memory".into()
    }

    fn read(&self) -> Result<Option<String>, OpError> {
        Ok(self.0.borrow().clone())
    }

    fn write(&self, content: &str) -> Result<(), OpError> {
        *self.0.borrow_mut() = Some(content.to_owned());
        Ok(())
    }

    fn remove(&self) -> Result<(), OpError> {
        *self.0.borrow_mut() = None;
        Ok(())
    }

    fn container_exists(&self) -> bool {
        true
    }

    fn remove_container(&self) -> Result<(), OpError> {
        Ok(())
    }
}

/// This platform's login item for `kilnd`, or `None` without a home directory (Linux).
#[must_use]
pub fn platform_item() -> Option<Rc<dyn LoginItem>> {
    #[cfg(windows)]
    {
        Some(Rc::new(RunValue::default()))
    }
    #[cfg(not(windows))]
    {
        FileItem::xdg_autostart().map(|item| Rc::new(item) as Rc<dyn LoginItem>)
    }
}

/// Lets the engine rebuild [`AutostartOp`]s from the journal, against `item`.
pub fn register_autostart(registry: &mut Registry, item: Rc<dyn LoginItem>) {
    registry.register(AUTOSTART_KIND, move |params| {
        let content = params["content"]
            .as_str()
            .ok_or_else(|| OpError::Failed("params.content missing".into()))?;
        Ok(Box::new(AutostartOp::new(
            content.to_owned(),
            Rc::clone(&item),
        )))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_ops::{Engine, MemoryJournal};

    fn engine(item: Rc<dyn LoginItem>) -> Engine<MemoryJournal> {
        let mut registry = Registry::default();
        register_autostart(&mut registry, item);
        Engine::new(MemoryJournal::default(), registry)
    }

    #[test]
    fn enables_and_undo_removes_it_again() {
        let item = Rc::new(MemoryItem::default());
        let mut engine = engine(item.clone());
        let op: Box<dyn Operation> = Box::new(AutostartOp::new("\"kilnd\"".into(), item.clone()));
        assert_eq!(op.preview().unwrap().changes[0].before, None);
        let report = engine.apply(&[op]).unwrap();
        assert_eq!(item.0.borrow().as_deref(), Some("\"kilnd\""));
        engine.revert(report.entry_ids[0], false).unwrap();
        assert_eq!(*item.0.borrow(), None);
    }

    #[test]
    fn undo_puts_back_a_replaced_entry_and_respects_hand_edits() {
        let item = Rc::new(MemoryItem(RefCell::new(Some("old".into()))));
        let mut engine = engine(item.clone());
        let op: Box<dyn Operation> = Box::new(AutostartOp::new("new".into(), item.clone()));
        let report = engine.apply(&[op]).unwrap();
        *item.0.borrow_mut() = Some("edited by hand".into());
        assert!(
            engine.revert(report.entry_ids[0], false).is_err(),
            "drifted"
        );
        assert_eq!(item.0.borrow().as_deref(), Some("edited by hand"));
        engine.revert(report.entry_ids[0], true).unwrap();
        assert_eq!(item.0.borrow().as_deref(), Some("old"));
    }

    #[test]
    fn a_file_item_and_the_folder_it_created_go_on_undo() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        std::fs::create_dir(&config).unwrap();
        let item = Rc::new(FileItem {
            path: config.join("autostart").join("kiln-kilnd.desktop"),
        });
        let mut engine = engine(item.clone());
        let content = desktop_entry(Path::new("/home/me/kiln/kilnd"));
        let op: Box<dyn Operation> = Box::new(AutostartOp::new(content.clone(), item.clone()));
        let report = engine.apply(&[op]).unwrap();
        assert_eq!(std::fs::read_to_string(&item.path).unwrap(), content);
        engine.revert(report.entry_ids[0], false).unwrap();
        assert!(!item.path.exists());
        assert!(
            !config.join("autostart").exists(),
            "the folder Kiln created"
        );
        assert!(config.is_dir(), "its parent stays");
    }

    #[test]
    fn a_folder_that_existed_or_holds_other_entries_stays() {
        let dir = tempfile::tempdir().unwrap();
        let autostart = dir.path().join("autostart");
        std::fs::create_dir(&autostart).unwrap();
        std::fs::write(autostart.join("other.desktop"), "x").unwrap();
        let item = Rc::new(FileItem {
            path: autostart.join("kiln-kilnd.desktop"),
        });
        let mut engine = engine(item.clone());
        let op: Box<dyn Operation> = Box::new(AutostartOp::new("entry".into(), item.clone()));
        let report = engine.apply(&[op]).unwrap();
        engine.revert(report.entry_ids[0], false).unwrap();
        assert!(autostart.join("other.desktop").exists());
    }

    #[test]
    fn desktop_entries_quote_the_path() {
        let entry = desktop_entry(Path::new("/opt/my \"tools\"/$kiln/kilnd"));
        assert!(
            entry.contains(r#"Exec="/opt/my \"tools\"/\$kiln/kilnd""#),
            "{entry}"
        );
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\n"));
    }
}
