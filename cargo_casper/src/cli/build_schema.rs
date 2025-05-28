mod artifact;

use std::{env::consts::DLL_EXTENSION, ffi::OsStr, io::Write, path::PathBuf};

use anyhow::Context;
use artifact::Artifact;
use cargo_metadata::MetadataCommand;

use crate::compilation::CompileJob;

/// The `build-schema` subcommand flow. The schema is written to the specified
/// [`Write`] implementer.
pub fn build_schema_impl<W: Write>(
    package_name: Option<&str>,
    output_writer: &mut W,
) -> Result<(), anyhow::Error> {
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
                .context("Root package not found in metadata")?,
            None => {
                let manifest_path_target = PathBuf::from("./Cargo.toml").canonicalize()?;
                metadata
                    .packages
                    .iter()
                    .find(|p| p.manifest_path.canonicalize().unwrap() == manifest_path_target)
                    .context("Root package not found in metadata")?
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

    if dependencies.contains(&"casper-macros".into()) {
        features.push("casper-macros/__abi_generator".to_owned());
    }

    let build_result = compilation
        .dispatch(env!("TARGET"), &features)
        .context("ABI-rich wasm compilation failure")?;

    // Extract ABI information from the built contract
    let artifact_path = build_result
        .artifacts()
        .iter()
        .find(|x| x.extension() == Some(OsStr::new(DLL_EXTENSION)))
        .context("Failed loading the built contract")?;

    let artifact = Artifact::from_path(artifact_path).context("Load library")?;
    let collected = artifact.collect_schema().context("Collect schema")?;
    serde_json::to_writer(output_writer, &collected).context("Serialize collected schema")?;
    Ok(())
}
