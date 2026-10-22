//! `kiln-shim`: installed as `php.exe` and `composer.exe` by `kiln shims install`. It runs the
//! PHP that Kiln uses for the current folder ([ADR 0016] §8–10).
//!
//! [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md

use kiln_runtime::shim::{self, Tool};

fn main() {
    let mut args = std::env::args_os();
    let exe = args
        .next()
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    // Use the real path when available: argv[0] may be just "php".
    let me = std::env::current_exe().unwrap_or(exe);
    let Some(tool) = Tool::from_exe(&me) else {
        eprintln!("kiln-shim runs as php.exe or composer.exe; `kiln shims install` sets that up");
        std::process::exit(2);
    };
    std::process::exit(shim::run(tool, args.collect()));
}
