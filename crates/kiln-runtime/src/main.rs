//! `kiln-shim`: installed as `php` and `composer` (`php.exe` and `composer.exe` on Windows) by
//! `kiln shims install`. It runs the PHP that Kiln uses for the current folder ([ADR 0016]
//! §8–10, [ADR 0023] §1).
//!
//! [ADR 0016]: ../../../docs/adr/0016-per-site-php-and-shims.md
//! [ADR 0023]: ../../../docs/adr/0023-linux-shims-and-path.md

use kiln_runtime::shim::{self, Tool};

fn main() {
    let mut args = std::env::args_os();
    let invoked = args
        .next()
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    // The name it was started under first: on Linux the real path (`/proc/self/exe`) resolves
    // links, so a link named `php` would otherwise look like `kiln-shim`. Then the real path,
    // for when argv[0] is something else.
    let tool = Tool::from_exe(&invoked).or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|me| Tool::from_exe(&me))
    });
    let Some(tool) = tool else {
        eprintln!("kiln-shim runs as php or composer; `kiln shims install` sets that up");
        std::process::exit(2);
    };
    std::process::exit(shim::run(tool, args.collect()));
}
