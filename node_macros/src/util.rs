use proc_macro2::Span;
use syn::Ident;

pub(crate) fn to_ident(s: &str) -> Ident {
    Ident::new(s, Span::call_site())
}

/// Returns an ident with a suffix.
pub(crate) fn suffix_ident(ident: &Ident, suffix: &str) -> Ident {
    to_ident(&format!("{}{}", ident, suffix))
}
