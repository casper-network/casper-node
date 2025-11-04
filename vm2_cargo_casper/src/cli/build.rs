use std::{fs::File, path::PathBuf, process::Command};

use anyhow::{anyhow, Context};
use casper_contract_sdk::{meta::Meta, serializers::borsh};

use crate::{cli::error::CliError, compilation::CompileJob, utils};

pub struct BuildResult {
    pub(crate) wasm: PathBuf,
    pub(crate) schema: Option<PathBuf>,
    pub(crate) meta: Option<PathBuf>,
}

/// The `build` subcommand flow.
pub fn build_impl(
    package_name: Option<&str>,
    output_dir: Option<PathBuf>,
    allow_skipping_abi_schema: bool,
) -> Result<BuildResult, anyhow::Error> {
    // Build the contract package targetting wasm32-unknown-unknown without
    // extra feature flags - this is the production contract wasm file.
    //
    // Optionally (but by default) create an entrypoint in the wasm that will have
    // embedded schema JSON file for discoverability (aka internal schema).
    let schema = match super::build_schema::build_schema_impl(package_name) {
        Ok(schema) => schema,
        Err(crate::cli::error::CliError::MissingRequiredFeatureSet)
            if allow_skipping_abi_schema =>
        {
            eprintln!(
                "🤷 Skipping ABI schema because the project doesn't have necessary dependencies..."
            );
            // Compile and move to specified output directory
            eprintln!("🔨 Fallback: Building contract WASM without ABI awareness...");
            let production_wasm_path = CompileJob::new(package_name, None, vec![])
                .dispatch("wasm32-unknown-unknown", Option::<String>::None)
                .context("Failed to compile user wasm")?
                .get_artifact_by_extension("wasm")
                .context("Failed extracting build artifacts to directory")?;

            return Ok(BuildResult {
                wasm: production_wasm_path,
                schema: None,
                meta: None,
            });
        }
        Err(other_error) => {
            return Err(other_error.into());
        }
    };

    // Build the contract with above schema injected
    eprintln!("🔨 Step 2: Building contract for wasm32-unknown_unknown...");
    let production_wasm_path = CompileJob::new(package_name, None, Vec::new())
        .dispatch("wasm32-unknown-unknown", Vec::<String>::new())
        .context("Failed to compile user wasm")?
        .get_artifact_by_extension("wasm")
        .context("Build artifacts for contract wasm didn't include a wasm file")?;

    // Run wasm optimizations passes that will shrink the size of the wasm.
    eprintln!("🔨 Step 3: Applying optimizations...");
    let strip_status = Command::new("wasm-strip")
        .arg(&production_wasm_path)
        .status()
        .context("Failed to execute wasm-strip command. Is wabt installed?")?;
    if !strip_status.success() {
        return Err(anyhow!(
            "wasm-strip command failed with status {strip_status}"
        ));
    }

    // schema
    let wasm_hash = utils::wasm::compute_wasm_hash(&production_wasm_path)?;
    eprintln!(
        "🔨 Step 3.5: Computing wasm hash for metadata... 0x{}",
        base16::encode_lower(&wasm_hash)
    );

    let meta = Meta::from_schema(schema.clone(), wasm_hash).map_err(|e| {
        CliError::SchemaConversionError(format!("Failed to convert schema to Meta: {}", e))
    })?;

    // Write the schema next to the wasm
    let schema_file_path = production_wasm_path.with_extension("json");
    let meta_file_path = production_wasm_path.with_extension("meta");

    std::fs::create_dir_all(schema_file_path.parent().unwrap())
        .context("Failed creating directory for wasm schema")?;

    serde_json::to_writer_pretty(&mut File::create(&schema_file_path)?, &schema)
        .context("Failed writing contract schema")?;
    borsh::to_writer(&mut File::create(&meta_file_path)?, &meta)
        .context("Failed writing contract meta file")?;

    // Move to output_dir if specified
    let mut out_wasm_path = production_wasm_path.clone();

    if let Some(output_dir) = output_dir {
        out_wasm_path = output_dir
            .join(out_wasm_path.file_stem().unwrap())
            .with_extension("wasm");
        std::fs::rename(&production_wasm_path, &out_wasm_path)
            .context("Couldn't write to the specified output directory.")?;
    }

    let out_schema_path = out_wasm_path.with_extension("json");
    let production_schema_path = production_wasm_path.with_extension("json");
    std::fs::rename(&production_schema_path, &out_schema_path)
        .context("Couldn't write to the specified output directory.")?;

    let out_meta_path = out_wasm_path.with_extension("meta");
    let production_meta_path = production_wasm_path.with_extension("meta");
    std::fs::rename(&production_meta_path, &out_meta_path)
        .context("Couldn't write to the specified output directory.")?;

    // Report paths
    eprintln!("✅ Completed.");

    Ok(BuildResult {
        wasm: out_wasm_path.canonicalize()?,
        schema: Some(out_schema_path),
        meta: Some(out_meta_path),
    })
}
