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

    pub fn display_with_features(&self, default_features: bool, features: Vec<&str>) -> String {
        let mut output = format!(r#"{} = {{ version = "{}""#, self.name, self.version);

        if let Some(workspace_path) = ARGS.workspace_path() {
            output = format!(
                r#"{}, path = "{}/{}""#,
                output,
                workspace_path.display(),
                self.relative_path
            );
        }

        if !default_features {
            output = format!("{}, default-features = false", output);
        }

        if !features.is_empty() {
            output = format!("{}, features = {:?}", output, features);
        }

        format!("{} }}", output)
    }

    #[cfg(test)]
    pub fn version(&self) -> &str {
        &self.version
    }
}
