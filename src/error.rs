use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid argument")]
    InvalidArgument,
    #[error("Command not found")]
    CommandNotFound,
    #[error("Failed to read directory")]
    ReadDirectoryFailed,
    #[error("IO Error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("Invalid UTF-8 string: {0}")]
    Utf8Error(#[from] std::string::FromUtf8Error),
    #[error("Invalid JSON format: {0}")]
    SerdeJsonError(#[from] serde_json::Error),
}
