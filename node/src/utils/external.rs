//! External resource handling
//!
//! The `External` type abstracts away the loading of external resources. See the type documentation
//! for details.

use std::{
    fmt,
    path::{Path, PathBuf},
};

use openssl::{
    pkey::{PKey, Private},
    x509::X509,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    crypto::{self, asymmetric_key::SecretKey},
    tls,
};

/// External resource.
///
/// An `External` resource can be given in two ways: Either as an immediate value, or through a
/// path, provided the value implements `Loadable`.
///
/// Serializing and deserializing an `External` value is only possible if it is in path form. This
/// is especially useful when writing structure configurations.
///
/// An `External` also always provides a default, which will always result in an error when `load`
/// is called. Should the underlying type `T` implement `Default`, the `load_with_default` can be
/// used instead.
#[derive(Clone, Eq, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub enum External<T> {
    /// Value that should be loaded from an external path.
    Path(PathBuf),
    /// Loaded or immediate value.
    #[serde(skip)]
    Loaded(T),
    /// The value has not been specified, but a default has been requested.
    #[serde(skip)]
    Missing,
}

impl<T> External<T> {
    /// Creates an external from a value.
    pub fn value(value: T) -> Self {
        External::Loaded(value)
    }

    /// Creates an external referencing a path.
    pub fn path<P: AsRef<Path>>(path: P) -> Self {
        External::Path(path.as_ref().to_owned())
    }
}

impl<T> External<T>
where
    T: Loadable,
{
    /// Loads the value if not loaded already, or returns available value.
    pub fn load(self) -> Result<T, LoadError<T::Error>> {
        match self {
            External::Path(path) => {
                T::from_file(&path).map_err(move |error| LoadError::Failed { error, path })
            }
            External::Loaded(value) => Ok(value),
            External::Missing => Err(LoadError::Missing),
        }
    }
}

impl<T> External<T>
where
    T: Loadable + Default,
{
    /// Loads the value if not loaded already, or returns available value.
    ///
    /// If missing, returns the default instead of an error.
    pub fn load_with_default(self) -> Result<T, LoadError<T::Error>> {
        match self {
            External::Missing => Ok(Default::default()),
            _ => self.load(),
        }
    }
}

/// A value that can be loaded from a file.
pub trait Loadable: Sized {
    /// Error that can occur when attempting to load.
    type Error: fmt::Debug + fmt::Display;

    /// Loads a value from the given input path.
    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error>;
}

impl<T> Default for External<T> {
    fn default() -> Self {
        External::Missing
    }
}

/// Error loading external value.
#[derive(Debug, Error)]
pub enum LoadError<E: fmt::Debug + fmt::Display> {
    /// Failed to load from path.
    #[error("could not load from {path}: {error}")]
    Failed { path: PathBuf, error: E },
    /// A value was missing.
    #[error("value is missing (default requested)")]
    Missing,
}

// We supply a few useful implementations for external types.
impl Loadable for X509 {
    type Error = anyhow::Error;

    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        tls::load_cert(path)
    }
}

impl Loadable for PKey<Private> {
    type Error = anyhow::Error;

    fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        tls::load_private_key(path)
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
