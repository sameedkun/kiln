//! The per-user Unix socket ([ADR 0026] §3) on each platform. Access control is the file
//! system: the user's profile ACL on Windows, a `0700` folder on Unix.
//!
//! [ADR 0026]: ../../../docs/adr/0026-kilnd-daemon-and-local-api.md

use std::io;
use std::path::Path;

/// A connected socket. Windows hands `AF_UNIX` connections over as [`std::net::TcpStream`],
/// which reads, writes and times out the same way.
#[cfg(windows)]
pub type Stream = std::net::TcpStream;
#[cfg(unix)]
pub type Stream = std::os::unix::net::UnixStream;

/// Connects to the daemon's socket at `path`.
pub fn connect(path: &Path) -> io::Result<Stream> {
    #[cfg(windows)]
    {
        kiln_sys::win::connect_unix(path)
    }
    #[cfg(unix)]
    {
        Stream::connect(path)
    }
}

/// The daemon's listening socket.
#[derive(Debug)]
pub struct Listener {
    #[cfg(windows)]
    inner: kiln_sys::win::UnixListener,
    #[cfg(unix)]
    inner: std::os::unix::net::UnixListener,
}

impl Listener {
    /// Binds `path` in a folder only this user can reach. A leftover socket file is removed
    /// first: the caller holds the daemon lock, so no other daemon is using it.
    pub fn bind(path: &Path) -> io::Result<Self> {
        if let Some(dir) = path.parent() {
            kiln_sys::background::create_private_dir(dir)?;
        }
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        #[cfg(windows)]
        let inner = kiln_sys::win::UnixListener::bind(path)?;
        #[cfg(unix)]
        let inner = std::os::unix::net::UnixListener::bind(path)?;
        Ok(Self { inner })
    }

    pub fn accept(&self) -> io::Result<Stream> {
        #[cfg(windows)]
        {
            self.inner.accept()
        }
        #[cfg(unix)]
        {
            self.inner.accept().map(|(stream, _)| stream)
        }
    }
}
