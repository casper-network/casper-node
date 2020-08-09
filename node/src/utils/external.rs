//! External resource handling
//!
//! The `External` type abstracts away the loading of external resources. See the type documentation
//! for details.
use serde::{Deserialize, Serialize};

use std::path::{Path, PathBuf};

/// External resource.
///
/// An `External` resource can be given in two ways: Either as an immediate value, or through a
/// path, provided the value implements `Loadable`.
///
/// Serializing and deserializing an `External` value is only possible if it is in path form. This
/// is especially useful when writing structure configurations.
#[derive(Clone, Eq, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum External<T> {
    /// A value that should be loaded from an external path.
    Path(PathBuf),
    /// A loaded value.
    #[serde(skip)]
    Loaded(T),
}

impl<T> External<T> {
    /// Creates an external from a value.
    pub fn from_value(value: T) -> Self {
        External::Loaded(value)
    }

    /// Creates an external referencing a path.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Self {
        External::Path(path.as_ref().to_owned())
    }
}

impl<T> External<T>
where
    T: Loadable,
{
    /// Loads the value if not loaded already, or returns available value.
    pub fn load(self) -> Result<T, T::Error> {
        match self {
            External::Path(path) => T::from_file(path),
            External::Loaded(value) => Ok(value),
        }
    }
}

/// A value that can be loaded from a file.
pub trait Loadable: Sized {
    type Error;

    /// Loads a value from the given input path.
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error>;
}

impl<T> Default for External<T>
where
    T: Default,
{
    fn default() -> Self {
        Self::Loaded(Default::default())
    }
}

#[cfg(test)]
mod tests {
    use super::External;

    #[test]
    fn test_to_string() {
        let val: External<()> = External::Path("foo/bar.toml".into());
        assert_eq!(
            "\"foo/bar.toml\"",
            serde_json::to_string(&val).expect("serialization error")
        );
    }

    #[test]
    fn test_load_from_string() {
        let input = "\"foo/bar.toml\"";

        let val: External<()> = serde_json::from_str(input).expect("deserialization failed");

        assert_eq!(External::Path("foo/bar.toml".into()), val);
    }
}
