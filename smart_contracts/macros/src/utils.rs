use quote::quote;
use syn::Signature;

const fn zero_exists_1d(arr: &[u8]) -> bool {
    if arr.is_empty() {
        return false;
    }

    let mut i = 0;
    while i < arr.len() {
        if arr[i] == 0 {
            return true;
        }
        i += 1;
    }

    false
}

const fn zero_exists_2d(arr: &[&[u8]]) -> bool {
    let mut i = 0;
    while i < arr.len() {
        if zero_exists_1d(arr[i]) {
            return true;
        }
        i += 1;
    }
    return false;
}

// const fn

/// Type definition without spaces
fn sanitized_type_name(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Tuple(tuple_type) => {
            let mut s = String::new();
            s.push_str("(");

            let types = tuple_type
                .elems
                .iter()
                .map(sanitized_type_name)
                .collect::<Vec<_>>()
                .join(",");
            s.push_str(&types);
            s.push_str(")");
            s
        }
        ty => {
            // TODO: Get other types as a string without spaces
            quote! { #ty }.to_string()
        }
    }
}

pub(crate) fn selector_preimage(signature: &Signature) -> String {
    let mut preimage = vec![signature.ident.to_string()];

    let mut inputs = signature.inputs.iter();

    let inputs = inputs
        .filter_map(|input| match input {
            syn::FnArg::Receiver(_) => None,
            syn::FnArg::Typed(typed) => match typed.pat.as_ref() {
                syn::Pat::Ident(ident) => Some(&typed.ty),
                _ => None,
            },
            _ => None,
        })
        .map(|token| sanitized_type_name(&token))
        .collect::<Vec<_>>(); //.map(|tok| tok.to_string()).join(",");

    preimage.push("(".to_string());

    preimage.push(inputs.join(","));

    preimage.push(")".to_string());

    preimage.join("")
}

pub(crate) fn compute_blake2b256(bytes: &[u8]) -> [u8; 32] {
    let mut context = blake2_rfc::blake2b::Blake2b::new(32);
    context.update(&bytes);
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

pub(crate) fn compute_selector_value(sig: &Signature) -> u32 {
    let preimage = selector_preimage(sig);
    let hash_bytes: blake2_rfc::blake2b::Blake2bResult = {
        let mut context = blake2_rfc::blake2b::Blake2b::new(32);
        context.update(preimage.as_bytes());
        context.finalize()
    };

    let selector_bytes: [u8; 4] = (&hash_bytes.as_bytes()[0..4]).try_into().unwrap();

    // Using be constructor from first 4 bytes in big endian order should basically copy first 4
    // bytes in order into the integer.
    u32::from_be_bytes(selector_bytes)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize() {
        let tuple = syn::parse_quote! {
            (u8, u32)
        };
        assert_eq!(super::sanitized_type_name(&tuple), "(u8,u32)".to_string());

        let unsigned_32 = syn::parse_quote! {
            u32
        };
        assert_eq!(super::sanitized_type_name(&unsigned_32), "u32".to_string());
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
