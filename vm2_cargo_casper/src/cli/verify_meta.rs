use std::{
    fs::File,
    io::BufReader,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, ensure, Context};
use casper_contract_sdk::{meta::Meta, schema::Schema, serializers::borsh::BorshDeserialize};

use crate::utils;

/// Verifies that the provided meta file aligns with the schema and wasm artifact.
pub fn verify_meta_impl(
    meta_path: PathBuf,
    schema_path: PathBuf,
    wasm_path: PathBuf,
) -> anyhow::Result<()> {
    eprintln!("🔍 Verifying meta file consistency...");

    let meta = load_meta(&meta_path)?;
    let schema = load_schema(&schema_path)?;
    let wasm_hash = utils::wasm::compute_wasm_hash(&wasm_path)
        .with_context(|| format!("Failed to compute hash from {}", wasm_path.display()))?;

    let meta_wasm_hash = match &meta {
        Meta::V1(v1) => *v1.wasm_hash(),
    };

    ensure!(
        meta_wasm_hash == wasm_hash,
        "WASM hash mismatch.\n  meta: 0x{}\n  wasm: 0x{}",
        base16::encode_lower(&meta_wasm_hash),
        base16::encode_lower(&wasm_hash)
    );

    let derived_meta = Meta::from_schema(schema, wasm_hash)
        .map_err(|e| anyhow!("Schema conversion failed: {e}"))?;

    ensure!(
        derived_meta == meta,
        "Schema-derived meta does not match the supplied .meta file. Rebuild the artifacts to refresh the meta file."
    );

    eprintln!("✅ Meta verification succeeded.");
    Ok(())
}

fn load_meta(path: &Path) -> anyhow::Result<Meta> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open meta file at {}", path.display()))?;
    let mut reader = BufReader::new(file);
    Meta::deserialize_reader(&mut reader)
        .with_context(|| format!("Failed to deserialize meta file at {}", path.display()))
}

fn load_schema(path: &Path) -> anyhow::Result<Schema> {
    let reader = BufReader::new(
        File::open(path)
            .with_context(|| format!("Failed to open schema file at {}", path.display()))?,
    );
    serde_json::from_reader(reader)
        .with_context(|| format!("Failed to parse schema JSON at {}", path.display()))
}
