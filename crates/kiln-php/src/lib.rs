//! PHP runtimes:
//!
//! - [`index`]: the windows.php.net release index; the x64 NTS build of each version
//!   ([ADR 0013]).
//! - [`install`]: verified download, safe extraction, `php.ini`, start-up check (Windows).
//! - [`ini`]: the generated `php.ini` section.
//! - [`linux`]: pinned static-php-cli builds or the system's PHP ([ADR 0022]).
//!
//! [ADR 0013]: ../../../docs/adr/0013-downloads-and-php-runtimes.md
//! [ADR 0022]: ../../../docs/adr/0022-linux-php-runtimes-and-fpm.md

pub mod index;
pub mod ini;
pub mod install;
pub mod linux;
pub mod xdebug;

pub use index::{INDEX_LIMIT, INDEX_URL, IndexError, PhpRelease};
pub use install::{InstallError, Installed, PHP_CGI_EXE, PHP_EXE, is_installed};
