use std::{io::Cursor, path::PathBuf, process::Command};

use anyhow::Context;

use crate::compilation::CompileJob;

/// The `build` subcommand flow.
pub fn build_impl(
    package_name: Option<&str>,
    output_dir: Option<PathBuf>,
    embed_schema: bool,
) -> Result<(), anyhow::Error> {
    // Build the contract package targetting wasm32-unknown-unknown without
    // extra feature flags - this is the production contract wasm file.
    //
    // Optionally (but by default) create an entrypoint in the wasm that will have
    // embedded schema JSON file for discoverability (aka internal schema).
    let production_wasm_path = if embed_schema {
        // Build the schema first
        let mut buffer = Cursor::new(Vec::new());
        super::build_schema::build_schema_impl(package_name, &mut buffer)
            .context("Failed to build contract schema")?;

        let contract_schema =
            String::from_utf8(buffer.into_inner()).context("Failed to read contract schema")?;

        // Build the contract with above schema injected
        eprintln!("🔨 Step 2: Building contract with schema injected...");
        let production_wasm_path = CompileJob::new(
            package_name,
            None,
            vec![("__CARGO_CASPER_INJECT_SCHEMA_MARKER", &contract_schema)],
        )
        .dispatch(
            "wasm32-unknown-unknown",
            ["casper-contract-sdk/__embed_schema"],
        )
        .context("Failed to compile user wasm")?
        .get_artifact_by_extension("wasm")
        .context("Build artifacts for contract wasm didn't include a wasm file")?;

        // Write the schema next to the wasm
        let schema_file_path = production_wasm_path.with_extension("json");

        std::fs::create_dir_all(schema_file_path.parent().unwrap())
            .context("Failed creating directory for wasm schema")?;

        std::fs::write(&schema_file_path, contract_schema)
            .context("Failed writing contract schema")?;

        production_wasm_path
    } else {
        // Compile and move to specified output directory
        eprintln!("🔨 Step 2: Building contract...");
        CompileJob::new(package_name, None, vec![])
            .dispatch("wasm32-unknown-unknown", Option::<String>::None)
            .context("Failed to compile user wasm")?
            .get_artifact_by_extension("wasm")
            .context("Failed extracting build artifacts to directory")?
    };

    // Run wasm optimizations passes that will shrink the size of the wasm.
    eprintln!("🔨 Step 3: Applying optimizations...");
    Command::new("wasm-strip")
        .args([&production_wasm_path])
        .spawn()
        .context("Failed to execute wasm-strip command. Is wabt installed?")?;

    // Move to output_dir if specified
    let mut out_wasm_path = production_wasm_path.clone();
    let mut out_schema_path = None;

    if let Some(output_dir) = output_dir {
        out_wasm_path = output_dir
            .join(out_wasm_path.file_stem().unwrap())
            .with_extension("wasm");
        std::fs::rename(&production_wasm_path, &out_wasm_path)
            .context("Couldn't write to the specified output directory.")?;
    }

    if embed_schema {
        out_schema_path = Some(out_wasm_path.with_extension("json"));
        let production_schema_path = production_wasm_path.with_extension("json");
        std::fs::rename(&production_schema_path, out_schema_path.as_ref().unwrap())
            .context("Couldn't write to the specified output directory.")?;
    }

    // Report paths
    eprintln!("✅ Completed. Build artifacts:");
    eprintln!("{:?}", out_wasm_path.canonicalize()?);
    if let Some(schema_path) = out_schema_path {
        eprintln!("{:?}", schema_path.canonicalize()?);
    }

    Ok(())
}
