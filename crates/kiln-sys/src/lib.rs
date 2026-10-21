//! Safe wrappers over OS APIs that need FFI. This is the only Kiln crate allowed to contain
//! `unsafe` ([ADR 0008](../../../docs/adr/0008-unsafe-confined-to-kiln-sys.md)); callers get
//! plain Rust types and `std::io::Result`.

mod replace;

pub use replace::{replace_file, write_replacing};
