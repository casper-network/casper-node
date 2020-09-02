use std::fmt::{Display, Formatter, Result};

use crate::ARGS;

/// Used to hold the information about the Casper dependencies which will be required by the
/// generated Cargo.toml files.
///
/// The information is output in a form suitable for injection into Cargo.toml via implementing the
/// `std::fmt::Display` trait.
#[derive(Debug)]
pub struct Dependency {
    name: String,
    version: String,
    /// Path relative to "casper-node"
    relative_path: String,
}

impl Dependency {
    pub fn new(name: &str, version: &str, relative_path: &str) -> Self {
        Dependency {
            name: name.to_string(),
            version: version.to_string(),
            relative_path: relative_path.to_string(),
        }
    }

    #[cfg(test)]
    pub fn version(&self) -> &str {
        &self.version
    }
}

impl Display for Dependency {
    fn fmt(&self, formatter: &mut Formatter) -> Result {
        if let Some(workspace_path) = ARGS.workspace_path() {
            write!(
                formatter,
                r#"{} = {{ version = "{}", path = "{}/{}" }}"#,
                self.name,
                self.version,
                workspace_path.display(),
                self.relative_path
            )
        } else {
            write!(formatter, r#"{} = "{}""#, self.name, self.version)
        }
    }
}
