//! Explicitly driven native transport. No executor or background threads.
//! Socket readiness, protocol progress, and deadlines belong to the caller.
pub use moq_core as moq;
mod quic;
pub mod runtime;
pub mod tls;
pub mod web;
pub use quic::*;

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("QUIC: {0}")]
    Quic(String),
    #[error("application closed ({code}): {reason}")]
    Closed { code: u64, reason: String },
    #[error("stream reset ({0})")]
    Reset(u64),
    #[error("stream stopped ({0})")]
    Stop(u64),
    #[error("HTTP/3 ({code}): {reason}")]
    Http3 { code: u64, reason: String },
    #[error("WebTransport: {0}")]
    Web(String),
}
impl Error {
    pub(crate) fn quic(e: impl std::fmt::Display) -> Self {
        Self::Quic(e.to_string())
    }
}
impl web_transport_trait::Error for Error {
    fn session_error(&self) -> Option<(u32, String)> {
        if let Self::Closed { code, reason } = self {
            Some(((*code).try_into().ok()?, reason.clone()))
        } else {
            None
        }
    }
    fn stream_error(&self) -> Option<u32> {
        if let Self::Reset(code) | Self::Stop(code) = self {
            (*code).try_into().ok()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    mod moq;
    mod quic;
    mod webtransport;
}
