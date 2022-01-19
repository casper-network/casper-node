use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use serde::Serialize;
use structopt::StructOpt;
use thiserror::Error;

/// Command processing error.
///
/// Failures that occur when trying to parse an incoming client message.
#[derive(Debug, Error)]
pub(super) enum Error {
    #[error("failed to split line using shell lexing rules")]
    ShlexFailure,
    #[error(transparent)]
    Invalid(#[from] structopt::clap::Error),
}

/// Output format information is sent back to the client it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub(super) enum OutputFormat {
    /// Human-readable interactive format.
    ///
    /// No string form, utilizes the `Display` implementation of types passed in.
    Interactive,
    /// JSON, pretty-printed.
    Json,
    /// Binary using bincode.
    Bincode,
}

impl Default for OutputFormat {
    fn default() -> Self {
        OutputFormat::Interactive
    }
}

impl Display for OutputFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            OutputFormat::Interactive => f.write_str("interactive"),
            OutputFormat::Json => f.write_str("json"),
            OutputFormat::Bincode => f.write_str("bincode"),
        }
    }
}

impl FromStr for OutputFormat {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "interactive" | "i" => Ok(OutputFormat::Interactive),
            "json" | "j" => Ok(OutputFormat::Json),
            "bincode" | "b" => Ok(OutputFormat::Bincode),
            _ => Err("invalid output format, must be one of 'interactive', 'json', 'bincode'"),
        }
    }
}

/// Action to perform.
#[derive(Debug, StructOpt)]
pub(super) enum Action {
    /// Retrieve the active console session information.
    Session,
    /// Set options on active console session.
    Set {
        /// Whether or not to omit command confirmation after every command sent. Defaults to off,
        /// meaning commands WILL send confirmations.
        #[structopt(short, long)]
        quiet: Option<bool>,
        /// Output format for any type of response, one of `interactive`, `json` or `bincode`.
        /// Defaults to `interactive`.
        #[structopt(short, long)]
        output: Option<OutputFormat>,
    },
    /// Dump the state of the consensus component.
    ///
    /// It is recommended to set the output format to `bincode` if the data is to be visualized
    /// after.
    DumpConsensus {
        /// Era to dump. If omitted, dumps the latest era.
        era: Option<u64>,
    },
}

/// A command to be performed on the node's console.
#[derive(Debug, StructOpt)]
pub(super) struct Command {
    #[structopt(subcommand)]
    pub(super) action: Action,
}

impl Command {
    /// Parses a line of input into a `Command`.
    pub(super) fn from_line(line: &str) -> Result<Self, Error> {
        let mut parts = vec!["casper-console".to_owned()];
        parts.extend(shlex::split(line).ok_or(Error::ShlexFailure)?);
        Ok(Self::from_iter_safe(parts.into_iter())?)
    }
}
