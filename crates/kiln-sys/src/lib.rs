//! Safe wrappers over OS APIs that need FFI. This is the only Kiln crate allowed to contain
//! `unsafe` ([ADR 0008](../../../docs/adr/0008-unsafe-confined-to-kiln-sys.md)); callers get
//! plain Rust types and `std::io::Result`.

pub mod background;
mod replace;
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod win;

pub use replace::{rename_replace, replace_file, write_replacing};
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub mod linux;
#[cfg(windows)]
#[allow(unsafe_code)]
pub mod wincrypt;
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
pub mod xattr;
