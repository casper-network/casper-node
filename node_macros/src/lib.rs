//! Generates reactors with routing from concise definitions. See `README.md` for details.

#![doc(html_root_url = "https://docs.rs/casper-node-macros/1.4.3")]
#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![warn(missing_docs, trivial_casts, trivial_numeric_casts)]

mod gen;
mod parse;
mod rust_type;
mod util;

use parse::{ByteSetterDefinition, ReactorDefinition};
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::quote;
use syn::parse_macro_input;

/// Generates a new reactor implementation, along with types.
#[proc_macro]
pub fn reactor(input: TokenStream) -> TokenStream {
    let mut def = parse_macro_input!(input as ReactorDefinition);

    // Insert the control announcements.
    def.inject_control_announcements();

    let mut output: proc_macro2::TokenStream = Default::default();

    output.extend(gen::generate_reactor(&def));
    output.extend(gen::generate_reactor_types(&def));
    output.extend(gen::generate_reactor_impl(&def));

    output.into()
}

/// Generates a function to set bytes in the ed25519 public key
#[proc_macro]
pub fn make_capnp_byte_setter_functions(input: TokenStream) -> TokenStream {
    let ByteSetterDefinition { length, builder } =
        parse_macro_input!(input as ByteSetterDefinition);
    let parsed_length: usize = length.base10_parse().expect("expected integer literal");

    let mut output: proc_macro2::TokenStream = Default::default();

    let mut inner_loop_set: proc_macro2::TokenStream = Default::default();
    for i in 0..parsed_length {
        let ident = Ident::new(&format!("set_byte{}", i), Span::call_site());
        inner_loop_set.extend(quote!(
            msg.#ident(bytes[#i]);
        ));
    }

    let builder_str = format!("public_key_capnp::{}_public_key::Builder", builder.value());
    let builder_stream: proc_macro2::TokenStream = builder_str.parse().expect("incorrect builder");

    let setter = Ident::new(&format!("set_{}", builder.value()), Span::call_site());
    output.extend(quote!(
        fn #setter(msg: &mut #builder_stream, bytes: &[u8; #length]) {
            #inner_loop_set
        }
    ));

    let mut inner_loop_get: proc_macro2::TokenStream = Default::default();
    for i in 0..parsed_length {
        let ident = Ident::new(&format!("get_byte{}", i), Span::call_site());
        inner_loop_get.extend(quote!(
            reader.#ident(),
        ));
    }

    let reader_str = format!("public_key_capnp::{}_public_key::Reader", builder.value());
    let reader_stream: proc_macro2::TokenStream = reader_str.parse().expect("incorrect builder");

    let getter = Ident::new(&format!("get_{}", builder.value()), Span::call_site());
    output.extend(quote!(
        fn #getter(reader: #reader_stream) -> [u8; #length] {
            [
                #inner_loop_get
            ]
        }
    ));

    output.into()
}
