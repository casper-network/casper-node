use std::{fs::File, io::Write};

use clap::Parser;
use cli::{Cli, Command};

pub(crate) mod cli;
pub(crate) mod compilation;
pub mod utils;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::BuildSchema { output, workspace } => {
            // If user specified an output path, write there.
            // Otherwise print to standard output.
            let mut schema_writer: Box<dyn Write> = match output {
                Some(path) => Box::new(File::create(path)?),
                None => Box::new(std::io::stdout()),
            };

            // Select the package to build
            let package_name = workspace.package.first().map(|x| x.as_str());

            cli::build_schema::build_schema_impl(package_name, &mut schema_writer)?
        }
        Command::Build {
            output,
            embed_schema,
            workspace,
        } => {
            // Select the package to build
            let package_name = workspace.package.first().map(|x| x.as_str());

            cli::build::build_impl(package_name, output, embed_schema.unwrap_or(true))?
        }
        Command::New { name } => cli::new::new_impl(&name)?,
    }
    Ok(())
}
