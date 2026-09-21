//! Direct LAN timecode synchronization with synchronous handles.
//!
//! ```no_run
//! use ethersync::{Engine, LeaderConfig};
//! let engine = Engine::new()?;
//! let leader = engine.leader(LeaderConfig::default())?;
//! leader.play()?;
//! let mut reader = leader.reader()?;
//! println!("{}", reader.read().label());
//! engine.shutdown()?;
//! # Ok::<(), ethersync::Error>(())
//! ```
pub use ethersync_protocol::{Boundary, BoundaryKind, FrameFormat, Label, Position, Rate};
mod api;
pub use ethersync_protocol::clock;
mod discovery;
mod network;
pub use api::*;
pub use discovery::*;
use ethersync_protocol::timeline;
use ethersync_protocol::tracking;
pub use timeline::{
    ConnectionState, Correction, CorrectionPolicy, Reading, SourceHealth, SourceKind, Status,
    SyncState,
};
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid configuration: {0}")]
    Invalid(&'static str),
    #[error("worker has shut down")]
    Shutdown,
    #[error("bounded command queue is full")]
    QueueFull,
    #[error("reader capacity reached")]
    ReaderLimit,
    #[error("protocol: {0}")]
    Protocol(#[from] ethersync_protocol::Error),
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("transport: {0}")]
    Transport(String),
    #[error("discovery: {0}")]
    Discovery(String),
}
pub type Result<T> = std::result::Result<T, Error>;

mod worker;
