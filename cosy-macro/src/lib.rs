//! Cosy reactor proc macro.
//!
//! Generates reactors with routing from concise definitions. See `README.md` for details.

mod gen;
mod parse;
mod rust_type;
mod util;

use proc_macro::TokenStream;
use syn::parse_macro_input;

use parse::ReactorDefinition;

/// Generates a new reactor implementation, along with types.
#[proc_macro]
pub fn reactor(input: TokenStream) -> TokenStream {
    let def = parse_macro_input!(input as ReactorDefinition);

    let mut output: proc_macro2::TokenStream = Default::default();

    output.extend(gen::generate_reactor(&def));
    output.extend(gen::generate_reactor_types(&def));
    output.extend(gen::generate_reactor_impl(&def));

    output.into()
}
