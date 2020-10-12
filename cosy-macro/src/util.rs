use inflector::cases::snakecase::to_snake_case;
use syn::{export::Span, Ident};

pub(crate) fn snake_ident(id: &Ident) -> Ident {
    to_ident(&to_snake_case(&id.to_string()))
}

pub(crate) fn to_ident(s: &str) -> Ident {
    Ident::new(s, Span::call_site())
}

pub(crate) fn suffix_ident(ident: &Ident, suffix: &str) -> Ident {
    /// Returns an ident with a suffix.
    to_ident(&format!("{}{}", ident.to_string(), suffix))
}
