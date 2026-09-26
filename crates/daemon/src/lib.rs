//! Per-repo Graphite daemon: owns the store, the derived adjacency and the watcher; serves thin clients over a unix socket.

pub mod client;
pub mod engine;
pub mod freshness;
pub mod info;
pub mod judge;
pub mod paths;
pub mod protocol;
pub mod queries;
pub mod search;
pub mod server;
pub mod watcher;

pub use engine::Engine;
pub use paths::RepoPaths;
pub use protocol::{Op, Request, Response};
pub use queries::{GraphQueries, Knobs, QueryHandler};
pub use search::SearchSpec;

/// Daemon error.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("store: {0}")]
    Store(#[from] graphite_store::StoreError),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("watch: {0}")]
    Watch(#[from] notify::Error),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("another daemon already owns {0}")]
    AlreadyRunning(String),
}

pub type Result<T> = std::result::Result<T, DaemonError>;
