use std::path::PathBuf;

use anyhow::Context;

use include_dir::{include_dir, Dir};

static TEMPLATE_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/project_template");
const TEMPLATE_NAME_MARKER: &str = "project_template";

/// The `new` subcommand flow.
pub fn new_impl(name: &str) -> Result<(), anyhow::Error> {
    let name = name
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-");

    let template_dir = super::extract_embedded_dir(&PathBuf::from(&name), &TEMPLATE_DIR)
        .context("Failed extracting template directory")?;

    let toml_path = template_dir.join("Cargo.toml");

    let toml_content = std::fs::read_to_string(&toml_path)
        .context("Failed reading template Cargo.toml file")?
        .replace(TEMPLATE_NAME_MARKER, &name);

    std::fs::write(toml_path, toml_content).context("Failed updating template Cargo.toml file")?;

    Ok(())
}
