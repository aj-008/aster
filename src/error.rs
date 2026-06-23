use thiserror::Error;

pub type Result<T> = std::result::Result<T, AsterError>;

#[derive(Error, Debug)]
pub enum AsterError {
    #[error("Failed to read or write data: {0}")]
    Io(#[from] std::io::Error),

    #[error("Invalid config: {0}")]
    Config(String),

    #[error("Trace parse error at instruction {instr}: {msg}")]
    TraceParse { instr: usize, msg: String },

}
