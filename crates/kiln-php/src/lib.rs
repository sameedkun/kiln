//! PHP runtimes ([ADR 0013]):
//!
//! - [`index`]: the windows.php.net release index; the x64 NTS build of each version.
//! - [`install`]: verified download, safe extraction, `php.ini`, start-up check.
//! - [`ini`]: the generated `php.ini` section.
//!
//! Windows builds only for now; Linux runtimes arrive with P1-04.
//!
//! [ADR 0013]: ../../../docs/adr/0013-downloads-and-php-runtimes.md

pub mod index;
pub mod ini;
pub mod install;

pub use index::{INDEX_LIMIT, INDEX_URL, IndexError, PhpRelease};
pub use install::{InstallError, Installed, PHP_CGI_EXE, PHP_EXE, is_installed};
