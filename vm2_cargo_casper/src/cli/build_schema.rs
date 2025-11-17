mod artifact;

use std::{env::consts::DLL_EXTENSION, ffi::OsStr, path::PathBuf};

use artifact::Artifact;
use cargo_metadata::MetadataCommand;
use casper_contract_sdk::schema::Schema;

use crate::{
    cli::{self, error::CliError},
    compilation::CompileJob,
};

/// The `build-schema` subcommand flow. The schema is written to the specified
/// [`Write`] implementer.
pub fn build_schema_impl(package_name: Option<&str>) -> cli::Result<Schema> {
    // Compile contract package to a native library with extra code that will
    // produce ABI information including entrypoints, types, etc.
    eprintln!("🔨 Step 1: Building contract schema...");

    let rustflags = {
        let current = std::env::var("RUSTFLAGS").unwrap_or_default();
        format!("-C link-dead-code {current}")
    };

    let compilation = CompileJob::new(package_name, None, vec![("RUSTFLAGS", &rustflags)]);

    // Get all of the direct user contract dependencies.
    //
    // This is a naive approach -- if a dep is feature gated, it won't be resolved correctly.
    // In practice, we only care about casper-contract-sdk and casper-macros being used, and there
    // is little to no reason to feature gate them. So this approach should be good enough.
    let dependencies: Vec<String> = {
        let metadata = MetadataCommand::new().exec()?;

        // Find the root package (the one whose manifest path matches our Cargo.toml)
        let package = match package_name {
            Some(package_name) => metadata
                .packages
                .iter()
                .find(|p| p.name == package_name)
                .ok_or(CliError::RootPackageNotFound)?,
            None => {
                let manifest_path_target = PathBuf::from("./Cargo.toml").canonicalize()?;
                metadata
                    .packages
                    .iter()
                    .find(|p| p.manifest_path.canonicalize().unwrap() == manifest_path_target)
                    .ok_or(CliError::RootPackageNotFound)?
            }
        };

        // Extract the direct dependency names from the package.
        package
            .dependencies
            .iter()
            .map(|dep| dep.name.clone())
            .collect()
    };

    // Determine extra features based on the dependencies detected
    let mut features = Vec::new();

    if dependencies.contains(&"casper-contract-sdk".into()) {
        features.push("casper-contract-sdk/__abi_generator".to_owned());
    }

    if dependencies.contains(&"casper-contract-macros".into()) {
        features.push("casper-contract-macros/__abi_generator".to_owned());
    }

    if features.is_empty() {
        return Err(CliError::MissingRequiredFeatureSet);
    }
    let build_result = compilation.dispatch(env!("TARGET"), &features)?;

    // Extract ABI information from the built contract
    let artifact_path = build_result
        .artifacts()
        .iter()
        .find(|x| x.extension() == Some(OsStr::new(DLL_EXTENSION)))
        .ok_or(CliError::NoCompiledArtifactFound)?;

    let artifact = Artifact::from_path(artifact_path)?;

    let collected = artifact.collect_schema()?;

    let schema: Schema = serde_json::from_value(collected).map_err(|e| {
        CliError::SchemaConversionError(format!("Failed to convert collected schema: {}", e))
    })?;

    Ok(schema)
}
