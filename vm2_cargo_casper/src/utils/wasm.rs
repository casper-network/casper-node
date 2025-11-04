use std::{io, path::Path};

use anyhow::Context;
use blake2::{digest::VariableOutput, Blake2bVar};

pub fn compute_wasm_hash(
    production_wasm_path: impl AsRef<Path>,
) -> Result<[u8; 32], anyhow::Error> {
    let mut hasher = Blake2bVar::new(32).unwrap();
    let file = std::fs::File::open(production_wasm_path.as_ref())
        .context("Failed opening produced wasm file for hashing")?;
    io::copy(&mut io::BufReader::new(file), &mut hasher)
        .context("Failed reading produced wasm file for hashing")?;
    let mut hash = [0u8; 32];
    hasher.finalize_variable(&mut hash).unwrap();
    Ok(hash)
}
