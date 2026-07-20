use thiserror::Error;

/// Result alias used throughout the crate; error type is always [`AsterError`].
pub type Result<T> = std::result::Result<T, AsterError>;

/// Coarse-grained category for an [`AsterError`], used where callers need to
/// branch on error type without matching the full enum (e.g. distinguishing
/// end-of-trace I/O from a real parse failure).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Io,
    Config,
    TraceParse,
    InvalidTrace,
    InvalidPolicyConfig,
}

/// Crate-wide error type covering config loading, trace parsing, and
/// replacement policy setup.
#[derive(Error, Debug)]
pub enum AsterError {
    #[error("Failed to read or write data: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid config: {0}")]
    Config(String),

    #[error("Trace parse error at instruction {instr}: {msg}")]
    TraceParse { instr: usize, msg: String },

    #[error("Unrecognized trace format {fmt}")]
    InvalidTrace { fmt: String },

    #[error("Invalid replacement policy config: {0}")]
    InvalidPolicyConfig(String),
}

impl AsterError {
    /// Returns the [`ErrorKind`] category for this error.
    pub fn kind(&self) -> ErrorKind {
        match self {
            AsterError::Io { .. } => ErrorKind::Io,
            AsterError::Config(_) => ErrorKind::Config,
            AsterError::TraceParse { .. } => ErrorKind::TraceParse,
            AsterError::InvalidTrace { .. } => ErrorKind::InvalidTrace,
            AsterError::InvalidPolicyConfig(_) => ErrorKind::InvalidPolicyConfig,
        }
    }
}
