use thiserror::Error;

pub type Result<T> = std::result::Result<T, AsterError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Io,
    Config,
    TraceParse,
    InvalidTrace,
    InvalidPolicyConfig,
}

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
