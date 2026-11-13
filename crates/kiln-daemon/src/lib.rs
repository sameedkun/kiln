//! `kilnd`, Kiln's optional per-user daemon ([ADR 0026]), and a client for its API.
//!
//! - [`proto`]: JSON-RPC 2.0 messages, one per line.
//! - [`transport`]: the per-user Unix socket on Windows and Unix.
//! - [`server`]: the accept loop, per-connection threads and event notifications.
//! - [`client`]: what `kiln daemon …` uses.
//! - [`daemon`]: the methods, the web-stack watchdog and the log.
//! - [`jobs`] and [`allow`]: running `kiln --json` for reads and jobs (API version 2).
//!
//! [ADR 0026]: ../../../docs/adr/0026-kilnd-daemon-and-local-api.md

pub mod allow;
pub mod client;
pub mod daemon;
pub mod jobs;
pub mod logs;
pub mod proto;
pub mod server;
pub mod transport;

pub use client::{Client, ClientError};
pub use daemon::{Daemon, EXE_NAME, Paths};
