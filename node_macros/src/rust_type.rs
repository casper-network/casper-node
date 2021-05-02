//! A full (but unchecked) Rust type, along with convenience methods.

use fmt::Debug;
use std::{
    convert::TryFrom,
    fmt::{self, Formatter},
};

use inflector::cases::snakecase::to_snake_case;
use proc_macro2::TokenStream;
use quote::quote;
use syn::{Ident, Path, Type};

use crate::util::to_ident;

/// A fully pathed Rust type with type arguments, e.g. `crate::components::SmallNet<NodeId>`.
pub(crate) struct RustType(Path);

impl Debug for RustType {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let path = &self.0;
        write!(f, "{}", quote!(#path).to_string())
    }
}

impl RustType {
    /// Creates a new `RustType` from a path.
    pub fn new(path: Path) -> Self {
        RustType(path)
    }

    /// Returns the types identifier without type arguments, e.g. `SmallNet`.
    pub fn ident(&self) -> Ident {
        self.0
            .segments
            .last()
            .expect("type has no last part?")
            .ident
            .clone()
    }

    /// Returns the type without the path, but with type arguments, e.g. `SmallNet<NodeId>`.
    pub fn ty(&self) -> TokenStream {
        let ident = self.ident();
        let args = &self
            .0
            .segments
            .last()
            .expect("type has no last part?")
            .arguments;
        quote!(#ident #args)
    }

    /// Returns the full type as it was given in the macro call.
    pub fn as_given(&self) -> &Path {
        &self.0
    }

    /// Returns the module name that canonically would contain the type, e.g. `small_net`.
    ///
    /// Based on the identifier only, i.e. will discard any actual path.
    pub fn module_ident(&self) -> Ident {
        to_ident(&to_snake_case(&self.ident().to_string()))
    }
}

impl TryFrom<Type> for RustType {
    type Error = String;

    fn try_from(value: Type) -> core::result::Result<Self, Self::Error> {
        match value {
            Type::Path(type_path) => Ok(RustType(type_path.path)),
            broken => Err(format!("cannot convert input {:?} to RustType", broken)),
        }
    }
}
