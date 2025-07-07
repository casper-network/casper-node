use std::{
    io,
    path::{Path, PathBuf},
};

use clap::Subcommand;
use include_dir::{Dir, DirEntry};

pub mod build;
pub mod build_schema;
pub mod new;

/// Writes the binary-embedded directory into a filesystem directory.
/// Returns the path to the extracted dir.
pub(crate) fn extract_embedded_dir(target: &Path, dir: &Dir) -> io::Result<PathBuf> {
    // Ensure the target directory exists.
    std::fs::create_dir_all(target)?;

    // Iterate over each entry in the directory.
    for entry in dir.entries() {
        match entry {
            DirEntry::File(file) => {
                let file_path = target.join(file.path());
                if let Some(parent) = file_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(file_path, file.contents())?;
            }
            DirEntry::Dir(sub_dir) => {
                extract_embedded_dir(target, sub_dir)?;
            }
        }
    }

    Ok(target.into())
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Build the JSON schema of the contract.
    BuildSchema {
        /// Where should the build artifacts be saved?
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// The cargo workspace
        #[command(flatten)]
        workspace: clap_cargo::Workspace,
    },
    /// Build the contract with its JSON schema embedded.
    Build {
        /// Where should the build artifacts be saved?
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Should the schema be embedded and exposed in the contract? (Default: true)
        #[arg(short, long)]
        embed_schema: Option<bool>,
        /// The cargo workspace
        #[command(flatten)]
        workspace: clap_cargo::Workspace,
    },
    /// Creates a new VM2 smart contract project from a template.
    New {
        /// Name of the project to create
        name: String,
    },
}

#[derive(Debug, clap::Parser)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub command: Command,
}
