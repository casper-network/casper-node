use std::path::{Path, PathBuf};

use crate::Hash;

/// Represents the types of session or payment code.
pub enum Code {
    /// The filesystem path of compiled Wasm code.
    Path(PathBuf),
    /// A named key providing the location of a stored contract.
    NamedKey(String, String),
    /// A hash providing the location of a stored contract.
    Hash(Hash, String),
}

// Note: can't just `impl<T: AsRef<Path>> From<T> for Code` because the compiler complains about
// a conflicting implementation of `From<URef>` - as URef could be made `AsRef<Path>` in the future

impl<'a> From<&'a str> for Code {
    fn from(path: &'a str) -> Code {
        Code::Path(path.into())
    }
}

impl<'a> From<&'a Path> for Code {
    fn from(path: &'a Path) -> Code {
        Code::Path(path.into())
    }
}

impl From<PathBuf> for Code {
    fn from(path: PathBuf) -> Code {
        Code::Path(path)
    }
}
