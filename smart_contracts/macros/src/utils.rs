use std::num::NonZeroU32;

use quote::quote;
use syn::Signature;

/// Type definition without spaces
fn sanitized_type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Tuple(tuple_type) => {
            let mut s = String::new();
            s.push('(');

            let types = tuple_type
                .elems
                .iter()
                .map(sanitized_type_name)
                .collect::<Vec<_>>()
                .join(",");
            s.push_str(&types);
            s.push(')');
            s
        }

        ty => {
            // TODO: Get other types as a string without spaces
            let mut s = quote! { #ty }.to_string();
            s = s.replace(' ', "");
            s
        }
    }
}

pub(crate) fn selector_preimage(signature: &Signature) -> String {
    let mut preimage: Vec<String> = vec![signature.ident.to_string()];

    let inputs = signature.inputs.iter();

    let inputs = inputs
        .filter_map(|input| match input {
            syn::FnArg::Receiver(_) => None,
            syn::FnArg::Typed(typed) => match typed.pat.as_ref() {
                syn::Pat::Ident(_ident) => Some(&typed.ty),
                _ => None,
            },
        })
        .map(|token| sanitized_type_name(token))
        .collect::<Vec<_>>();

    preimage.push('('.to_string());

    preimage.push(inputs.join(","));

    preimage.push(")".to_string());

    preimage.join("")
}

pub(crate) fn compute_blake2b256(bytes: &[u8]) -> [u8; 32] {
    let mut context = blake2_rfc::blake2b::Blake2b::new(32);
    context.update(bytes);
    context.finalize().as_bytes().try_into().unwrap()
}

pub(crate) fn compute_selector_bytes(bytes: &[u8]) -> u32 {
    let hash_bytes: blake2_rfc::blake2b::Blake2bResult = {
        let mut context = blake2_rfc::blake2b::Blake2b::new(32);
        context.update(bytes);
        context.finalize()
    };

    let selector_bytes: [u8; 4] = (&hash_bytes.as_bytes()[0..4]).try_into().unwrap();

    // Using be constructor from first 4 bytes in big endian order should basically copy first 4
    // bytes in order into the integer.
    u32::from_be_bytes(selector_bytes)
}

pub(crate) fn compute_selector_value(sig: &Signature) -> NonZeroU32 {
    let preimage = selector_preimage(sig);
    let hash_bytes: blake2_rfc::blake2b::Blake2bResult = {
        let mut context = blake2_rfc::blake2b::Blake2b::new(32);
        context.update(preimage.as_bytes());
        context.finalize()
    };

    let selector_bytes: [u8; 4] = (&hash_bytes.as_bytes()[0..4]).try_into().unwrap();

    // Using be constructor from first 4 bytes in big endian order should basically copy first 4
    // bytes in order into the integer.
    let value = u32::from_be_bytes(selector_bytes);
    // The chances of this being zero are astronomically low.
    NonZeroU32::new(value).expect("Computed selector value should not be zero")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize() {
        let tuple = syn::parse_quote! {
            (u8, u32, Option<(String, u64)>)
        };
        assert_eq!(
            super::sanitized_type_name(&tuple),
            "(u8,u32,Option<(String,u64)>)".to_string()
        );

        let unsigned_32 = syn::parse_quote! {
            u32
        };
        assert_eq!(super::sanitized_type_name(&unsigned_32), "u32".to_string());

        let result_ty = syn::parse_quote! {
            Result<u32, Error>
        };
        assert_eq!(
            super::sanitized_type_name(&result_ty),
            "Result<u32,Error>".to_string()
        );
    }
    #[test]
    fn test_selector_preimage() {
        let foo_function = syn::parse_quote! {
            fn foo_function(arg:(u8, u32)) -> u32
        };
        let my_function = syn::parse_quote! {
            fn my_function(arg1: u32, arg2: String) -> u64
        };
        let my_function_with_receiver = syn::parse_quote! {
            fn my_function_with_receiver(&mut self) -> String
        };
        assert_eq!(selector_preimage(&foo_function), "foo_function((u8,u32))");
        assert_eq!(selector_preimage(&my_function), "my_function(u32,String)");
        assert_eq!(
            selector_preimage(&my_function_with_receiver),
            "my_function_with_receiver()"
        );
    }
}
