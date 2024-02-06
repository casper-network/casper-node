mod validation;

use crate::{schema::CasperSchema, Contract};
use casper_sdk_sys::EntryPoint;
use clap::{error::ErrorKind, Parser, Subcommand};
use thiserror::Error;
use vm_common::flags::EntryPointFlags;

use self::validation::Validation;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error(transparent)]
    Parse(#[from] clap::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl Error {
    /// Exit the process with an error message.
    pub fn exit(&self) -> ! {
        match self {
            Error::Parse(clap_error) => {
                clap_error.exit();
            }
            Error::Json(serde_json_error) => {
                eprintln!("JSON error: {}", serde_json_error);
                std::process::exit(1);
            }
        }
    }
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Dump the schema of the contract to standard output.
    DumpSchema,
    /// Validate the contract against set of rules.
    Check,
}

#[derive(Debug, Parser)]
#[command(
    about,
    long_about = "Command-line tool for smart contracts",
    arg_required_else_help = true
)]
struct Cli {
    /// Turn debugging information on
    #[arg(short, long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[command(subcommand)]
    command: Option<Commands>,
}

pub fn try_command_line<T: Contract + CasperSchema>() -> Result<(), Error> {
    let cli = Cli::try_parse()?;
    match cli.command {
        Some(Commands::DumpSchema) => {
            let schema = T::schema();
            let pretty_json_schema = serde_json::to_string_pretty(&schema)?;
            println!("{}", pretty_json_schema);
        }
        Some(Commands::Check) => {
            let validations = {
                let schema = <T>::schema();

                let mut validations = Vec::new();

                if cli.verbose > 0 {
                    println!(
                        "Counting number of entry points in contract... {}",
                        schema.entry_points.len()
                    );
                }

                if schema.entry_points.is_empty() {
                    validations.push(Validation::NoEntryPoints);
                }

                validations
            };

            if !validations.is_empty() {
                for violation in validations {
                    eprintln!("Validation error: {}", violation);
                }
                std::process::exit(1);
            }
        }
        None => {}
    }
    Ok(())
}

pub fn command_line<T: Contract + CasperSchema>() {
    if let Err(error) = try_command_line::<T>() {
        error.exit()
    }
}

pub fn check_contract<T: Contract + CasperSchema>() -> Vec<Validation> {
    let schema = T::schema();

    let mut validations = Vec::new();

    // Check existence of a constructor
    if schema.entry_points.is_empty() {
        validations.push(Validation::NoEntryPoints);
    }

    validations
}
