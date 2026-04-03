use thiserror::Error;

#[derive(Debug, Error)]
pub enum PrivateError {
    #[error("key error: {0}")]
    InvalidKey(String),

    #[error("parse error: {0}")]
    ParseError(String),

    #[error("policy denied: {0}")]
    PolicyDenied(String),

    #[error("signing error: {0}")]
    SigningError(String),

    #[error("aztec error: {0}")]
    AztecError(String),

    #[error("hex decode error: {0}")]
    HexError(#[from] hex::FromHexError),
}
