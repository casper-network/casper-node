use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Validation {
    #[error("Contract does not have any entry points")]
    NoEntryPoints,
}
