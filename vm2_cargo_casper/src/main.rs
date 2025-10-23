use std::{fs::File, io::Write};

use clap::Parser;
use cli::{Cli, Command};

use crate::cli::error::CliError;

pub(crate) mod cli;
pub(crate) mod compilation;
pub mod utils;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::BuildSchema {
            output,
            workspace,
            allow_skipping_abi_schema,
        } => {
            // If user specified an output path, write there.
            // Otherwise print to standard output.
            let mut schema_writer: Box<dyn Write> = match output.clone() {
                Some(path) => Box::new(File::create(path)?),
                None => Box::new(std::io::stdout()),
            };

            let mut bundle_writer: Box<dyn Write> = match output.map(|p| p.with_extension("bundle"))
            {
                Some(path) => Box::new(File::create(path)?),
                None => Box::new(std::io::empty()),
            };

            // Select the package to build
            let package_name = workspace.package.first().map(|x| x.as_str());

            match cli::build_schema::build_schema_impl(
                package_name,
                &mut schema_writer,
                &mut bundle_writer,
            ) {
                Ok(_) => {}
                Err(CliError::MissingRequiredFeatureSet) if allow_skipping_abi_schema => {
                    eprintln!(
                        "🤷 Skipping ABI schema because the project doesn't have necessary dependencies..."
                    );
                    std::process::exit(1);
                }
                Err(other_error) => {
                    return Err(other_error.into());
                }
            }
        }
        Command::Build {
            output,
            embed_schema,
            workspace,
            allow_skipping_abi_schema,
        } => {
            // Select the package to build
            let package_name = workspace.package.first().map(|x| x.as_str());

            let build_result = cli::build::build_impl(
                package_name,
                output,
                embed_schema.unwrap_or(true),
                allow_skipping_abi_schema,
            )?;
            println!("WASM built at: {}", build_result.wasm.display());
            if let Some(schema_path) = build_result.schema {
                println!("Schema built at: {}", schema_path.display());
            }
            if let Some(bundle_path) = build_result.bundle {
                println!("Bundle built at: {}", bundle_path.display());
            }
        }
        Command::New { name } => cli::new::new_impl(&name)?,
    }
    Ok(())
}
