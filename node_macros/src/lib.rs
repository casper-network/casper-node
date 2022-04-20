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

use casper_types::PublicKey;
use proc_macro::TokenStream;
use proc_macro2::{Ident, Span};
use quote::quote;
use syn::parse_macro_input;

use parse::ReactorDefinition;

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
pub fn make_ed25519_capnp_functions(_input: TokenStream) -> TokenStream {
    let mut output: proc_macro2::TokenStream = Default::default();

    let mut inner_loop_set: proc_macro2::TokenStream = Default::default();
    for i in 0..PublicKey::ED25519_LENGTH {
        let ident = Ident::new(&format!("set_byte{}", i), Span::call_site());
        inner_loop_set.extend(quote!(
            msg.#ident(bytes[#i]);
        ));
    }

    output.extend(quote!(
        fn set_ed25519(msg: &mut public_key_capnp::ed25519_public_key::Builder, bytes: &[u8; PublicKey::ED25519_LENGTH]) {
            #inner_loop_set
        }
    ));

    let mut inner_loop_get: proc_macro2::TokenStream = Default::default();
    for i in 0..PublicKey::ED25519_LENGTH {
        let ident = Ident::new(&format!("get_byte{}", i), Span::call_site());
        inner_loop_get.extend(quote!(
            reader.#ident(),
        ));
    }

    output.extend(quote!(
        fn get_ed25519(reader: public_key_capnp::ed25519_public_key::Reader) -> [u8; PublicKey::ED25519_LENGTH] {
            [
                #inner_loop_get
            ]
        }
    ));

    output.into()
}

/// Generates a function to set bytes in the SECP256K1 public key
// TODO[RC]: Deduplicate with `make_ed25519_capnp_functions`
#[proc_macro]
pub fn make_secp256k1_capnp_functions(_input: TokenStream) -> TokenStream {
    let mut output: proc_macro2::TokenStream = Default::default();

    let mut inner_loop_set: proc_macro2::TokenStream = Default::default();
    for i in 0..PublicKey::SECP256K1_LENGTH {
        let ident = Ident::new(&format!("set_byte{}", i), Span::call_site());
        inner_loop_set.extend(quote!(
            msg.#ident(bytes[#i]);
        ));
    }

    output.extend(quote!(
        fn set_secp256k1(msg: &mut public_key_capnp::secp256k1_public_key::Builder, bytes: &[u8; PublicKey::SECP256K1_LENGTH]) {
            #inner_loop_set
        }
    ));

    let mut inner_loop_get: proc_macro2::TokenStream = Default::default();
    for i in 0..PublicKey::SECP256K1_LENGTH {
        let ident = Ident::new(&format!("get_byte{}", i), Span::call_site());
        inner_loop_get.extend(quote!(
            reader.#ident(),
        ));
    }

    output.extend(quote!(
        fn get_secp256k1(reader: public_key_capnp::secp256k1_public_key::Reader) -> [u8; PublicKey::SECP256K1_LENGTH] {
            [
                #inner_loop_get
            ]
        }
    ));

    output.into()
}

/// Generates functions to get/set bytes in capnp::U512
// TODO: Add byte length constant in casper_types::U512 to replace 64usize.
#[proc_macro]
pub fn make_u512_capnp_functions(_input: TokenStream) -> TokenStream {
    let mut output: proc_macro2::TokenStream = Default::default();

    let mut inner_loop_set: proc_macro2::TokenStream = Default::default();
    for i in 0..64usize {
        let ident = Ident::new(&format!("set_byte{}", i), Span::call_site());
        inner_loop_set.extend(quote!(
            msg.#ident(bytes[#i]);
        ));
    }

    output.extend(quote!(
        fn set_u512(msg: &mut common_capnp::u512::Builder, bytes: &[u8; 64usize]) {
            #inner_loop_set
        }
    ));

    let mut inner_loop_get: proc_macro2::TokenStream = Default::default();
    for i in 0..64usize {
        let ident = Ident::new(&format!("get_byte{}", i), Span::call_site());
        inner_loop_get.extend(quote!(
            reader.#ident(),
        ));
    }

    output.extend(quote!(
        fn get_u512(reader: &common_capnp::u512::Reader) -> [u8; 64usize] {
            [
                #inner_loop_get
            ]
        }
    ));

    output.into()
}
