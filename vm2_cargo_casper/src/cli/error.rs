use thiserror::Error;

use crate::utils::command_runner;

/// Common CLI error type for this crate.
#[derive(Debug, Error)]
pub enum CliError {
    /// This variant is reported whenever a Cargo.toml file does not have necessary dependencies to
    /// be considered a valid smart contract.
    ///
    /// This will cause a plain wasm32 build without the necessary information to provide a JSON
    /// schema and bundle.
    #[error("Missing feature set specification")]
    MissingRequiredFeatureSet,

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    CargoMetadata(#[from] cargo_metadata::Error),

    #[error("Root package not found in Cargo metadata")]
    RootPackageNotFound,

    #[error("No compiled artifact found after build")]
    NoCompiledArtifactFound,

    #[error(transparent)]
    Loading(#[from] libloading::Error),

    #[error(transparent)]
    CommandRunner(#[from] command_runner::Outcome),
}
