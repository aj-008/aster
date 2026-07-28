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

#[cfg(test)]
mod tests {
    use super::*;

    fn io_error() -> AsterError {
        std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "eof").into()
    }

    #[test]
    fn kind_maps_each_variant() {
        assert_eq!(io_error().kind(), ErrorKind::Io);
        assert_eq!(AsterError::Config("x".into()).kind(), ErrorKind::Config);
        assert_eq!(
            AsterError::TraceParse {
                instr: 0,
                msg: "x".into()
            }
            .kind(),
            ErrorKind::TraceParse
        );
        assert_eq!(
            AsterError::InvalidTrace { fmt: "x".into() }.kind(),
            ErrorKind::InvalidTrace
        );
        assert_eq!(
            AsterError::InvalidPolicyConfig("x".into()).kind(),
            ErrorKind::InvalidPolicyConfig
        );
    }

    #[test]
    fn io_from_conversion_preserves_message() {
        let err: AsterError = std::io::Error::new(std::io::ErrorKind::NotFound, "missing").into();
        assert!(err.to_string().contains("missing"));
        assert_eq!(err.kind(), ErrorKind::Io);
    }

    #[test]
    fn display_messages_include_payload() {
        assert_eq!(
            AsterError::Config("bad block_size".to_string()).to_string(),
            "Invalid config: bad block_size"
        );
        assert_eq!(
            AsterError::TraceParse {
                instr: 42,
                msg: "short read".to_string()
            }
            .to_string(),
            "Trace parse error at instruction 42: short read"
        );
        assert_eq!(
            AsterError::InvalidTrace {
                fmt: "foo".to_string()
            }
            .to_string(),
            "Unrecognized trace format foo"
        );
        assert_eq!(
            AsterError::InvalidPolicyConfig("nope".to_string()).to_string(),
            "Invalid replacement policy config: nope"
        );
    }

    #[test]
    fn error_kind_variants_are_distinct() {
        let kinds = [
            ErrorKind::Io,
            ErrorKind::Config,
            ErrorKind::TraceParse,
            ErrorKind::InvalidTrace,
            ErrorKind::InvalidPolicyConfig,
        ];
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }
}
