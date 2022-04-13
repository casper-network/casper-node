//! External resource handling
//!
//! The `External` type abstracts away the loading of external resources. See the type documentation
//! for details.

use std::{
    fmt::{Debug, Display},
    path::{Path, PathBuf},
    sync::Arc,
};

use casper_types::crypto;
use datasize::DataSize;
#[cfg(test)]
use once_cell::sync::Lazy;
use openssl::{
    pkey::{PKey, Private},
    x509::X509,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use casper_types::{
    file_utils::{read_file, ReadFileError},
    SecretKey,
};

use crate::tls;

/// Path to bundled resources.
#[cfg(test)]
pub static RESOURCES_PATH: Lazy<PathBuf> =
    Lazy::new(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../resources"));

/// External resource.
///
/// An `External` resource can be given in two ways: Either as an immediate value, or through a
/// path, provided the value implements `Loadable`.
///
/// Serializing and deserializing an `External` value is only possible if it is in path form. This
/// is especially useful when writing structure configurations.
///
/// An `External` also always provides a default, which will always result in an error when `load`
/// is called. Should the underlying type `T` implement `Default`, the `with_default` can be
/// used instead.
#[derive(Clone, DataSize, Eq, Debug, Deserialize, PartialEq, Serialize)]
#[serde(untagged)]
pub(crate) enum External {
    /// Value that should be loaded from an external path.
    Path(PathBuf),
    /// The value has not been specified, but a default has been requested.
    #[serde(skip)]
    Missing,
}

impl External {
    /// Loads the value if not loaded already, resolving relative paths from `root` or returns
    /// available value. If the value is `Missing`, returns an error.
    pub fn load<T, P>(self, root: P) -> Result<T, LoadError<T::Error>>
    where
        T: Loadable,
        P: AsRef<Path>,
    {
        match self {
            External::Path(path) => {
                let full_path = if path.is_relative() {
                    root.as_ref().join(&path)
                } else {
                    path
                };

                T::from_path(&full_path).map_err(move |error| LoadError::Failed {
                    error,
                    // We canonicalize `full_path` here, with `ReadFileError` we get extra
                    // information about the absolute path this way if the latter is relative. It
                    // will still be relative if the current path does not exist.
                    path: full_path.canonicalize().unwrap_or(full_path),
                })
            }
            External::Missing => Err(LoadError::Missing),
        }
    }
}

/// A value that can be loaded from a file.
pub trait Loadable: Sized {
    /// Error that can occur when attempting to load.
    type Error: Debug + Display;

    /// Loads a value from the given input path.
    fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error>;

    /// Load a test-only instance from the local path.
    #[cfg(test)]
    fn from_resources<P: AsRef<Path>>(rel_path: P) -> Self {
        Self::from_path(RESOURCES_PATH.join(rel_path.as_ref())).unwrap_or_else(|error| {
            panic!(
                "could not load resources from {}: {}",
                rel_path.as_ref().display(),
                error
            )
        })
    }
}

impl Default for External {
    fn default() -> Self {
        External::Missing
    }
}

fn display_res_path<E>(result: &Result<PathBuf, E>) -> String {
    result
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| String::new())
}

/// Error loading external value.
#[derive(Debug, Error)]
pub enum LoadError<E: Debug + Display> {
    /// Failed to load from path.
    #[error("could not load from {}: {error}", display_res_path(&.path.canonicalize()))]
    Failed {
        /// Path that failed to load.
        path: PathBuf,
        /// Error load failed with.
        error: E,
    },
    /// A value was missing.
    #[error("value is missing (default requested)")]
    Missing,
}

// We supply a few useful implementations for external types.
impl Loadable for X509 {
    type Error = anyhow::Error;

    fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        tls::load_cert(path)
    }
}

impl Loadable for PKey<Private> {
    type Error = anyhow::Error;

    fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        tls::load_private_key(path)
    }
}

impl Loadable for Arc<SecretKey> {
    type Error = crypto::ErrorExt;

    fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        Ok(Arc::new(SecretKey::from_file(path)?))
    }
}

impl Loadable for Vec<u8> {
    type Error = ReadFileError;

    fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, Self::Error> {
        read_file(path)
    }
}

#[cfg(test)]
mod tests {
    use super::External;

    #[test]
    fn test_to_string() {
        let val: External = External::Path("foo/bar.toml".into());
        assert_eq!(
            "\"foo/bar.toml\"",
            serde_json::to_string(&val).expect("serialization error")
        );
    }

    #[test]
    fn test_load_from_string() {
        let input = "\"foo/bar.toml\"";

        let val: External = serde_json::from_str(input).expect("deserialization failed");

        assert_eq!(External::Path("foo/bar.toml".into()), val);
    }
}
