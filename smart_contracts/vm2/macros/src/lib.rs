pub(crate) mod utils;

extern crate proc_macro;

use darling::{ast, util::Override, FromAttributes, FromMeta};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote, ToTokens};
use syn::{
    parse_macro_input, DeriveInput, Fields, ItemEnum, ItemFn, ItemImpl, ItemStruct, ItemTrait,
    ItemUnion, LitStr, Type,
};

#[derive(Debug, FromAttributes)]
#[darling(attributes(casper))]
struct MethodAttribute {
    #[darling(default)]
    constructor: bool,
    /// Does not read or write state and does not require "self" argument.
    #[darling(default)]
    ignore_state: bool,
    #[darling(default)]
    rollback_on_error: bool,
    /// Explicitly mark method as private so it's not externally callable.
    #[darling(default)]
    private: bool,
    #[darling(default)]
    payable: bool,
    #[darling(default)]
    abi_convention: Option<syn::Path>,
}
#[derive(Debug, Clone, FromMeta)]
struct MessageMeta {
    topic: String,
}

#[derive(Debug, FromMeta)]
struct StructMeta {
    #[darling(default)]
    path: Option<syn::Path>,
    /// Contract state is a special struct that is used to store the state of the contract.
    #[darling(default)]
    contract_state: bool,
    /// Message is a special struct that is used to send messages to other contracts.
    #[darling(default)]
    message: Option<Override<MessageMeta>>,
    #[darling(default)]
    abi_convention: Option<syn::Path>,
}

#[derive(Debug, FromMeta)]
struct EnumMeta {
    #[darling(default)]
    path: Option<syn::Path>,
}

#[derive(Debug, FromMeta)]
struct TraitMeta {
    path: Option<syn::Path>,
    export: Option<bool>,
    #[darling(default)]
    abi_convention: Option<syn::Path>,
}

#[derive(Debug, FromMeta)]
enum ItemFnMeta {
    Export,
}

#[derive(Debug, FromMeta)]
struct ImplTraitForContractMeta {
    /// Fully qualified path of the trait.
    #[darling(default)]
    path: Option<syn::Path>,
    /// Does not produce Wasm exports for the entry points.
    #[darling(default)]
    compile_as_dependency: bool,
}

fn generate_call_data_return(output: &syn::ReturnType) -> proc_macro2::TokenStream {
    match output {
        syn::ReturnType::Default => {
            quote! { () }
        }
        syn::ReturnType::Type(_, ty) => match ty.as_ref() {
            Type::Never(_) => {
                quote! { () }
            }
            Type::Reference(reference) => {
                // ty.uses_lifetimes(options, lifetimes)
                let mut new_ref = reference.clone();
                new_ref.lifetime = Some(syn::Lifetime::new("'a", Span::call_site()));
                quote! { <<#new_ref as core::ops::Deref>::Target as casper_contract_sdk::prelude::borrow::ToOwned>::Owned }
            }
            _ => {
                quote! { #ty }
            }
        },
    }
}

#[proc_macro_attribute]
pub fn casper(attrs: TokenStream, item: TokenStream) -> TokenStream {
    // let attrs: Meta = parse_macro_input!(attrs as Meta);
    let attr_args = match ast::NestedMeta::parse_meta_list(attrs.into()) {
        Ok(v) => v,
        Err(e) => {
            return TokenStream::from(e.to_compile_error());
        }
    };

    if let Ok(item_struct) = syn::parse::<ItemStruct>(item.clone()) {
        let struct_meta = StructMeta::from_list(&attr_args).unwrap();
        if struct_meta.message.is_some() {
            process_casper_message_for_struct(&item_struct, struct_meta)
        } else if struct_meta.contract_state {
            // #[casper(contract_state)]
            process_casper_contract_state_for_struct(&item_struct, struct_meta)
        } else {
            // For any other struct that will be part of a schema
            // #[casper]
            let partial = generate_casper_state_for_struct(&item_struct, struct_meta);
            quote! {
                #partial
            }
            .into()
        }
    } else if let Ok(item_enum) = syn::parse::<ItemEnum>(item.clone()) {
        let enum_meta = EnumMeta::from_list(&attr_args).unwrap();
        let partial = generate_casper_state_for_enum(&item_enum, enum_meta);
        quote! {
            #partial
        }
        .into()
    } else if let Ok(item_trait) = syn::parse::<ItemTrait>(item.clone()) {
        let trait_meta = TraitMeta::from_list(&attr_args).unwrap();
        casper_trait_definition(item_trait, trait_meta)
    } else if let Ok(entry_points) = syn::parse::<ItemImpl>(item.clone()) {
        if let Some((_not, trait_path, _for)) = entry_points.trait_.as_ref() {
            let impl_meta = ImplTraitForContractMeta::from_list(&attr_args).unwrap();
            generate_impl_trait_for_contract(&entry_points, trait_path, impl_meta)
        } else {
            generate_impl_for_contract(entry_points)
        }
    } else if let Ok(func) = syn::parse::<ItemFn>(item.clone()) {
        let func_meta = ItemFnMeta::from_list(&attr_args).unwrap();
        match func_meta {
            ItemFnMeta::Export => generate_export_function(&func),
        }
    } else {
        let err = syn::Error::new(
            Span::call_site(),
            "State attribute can only be applied to struct or enum",
        );
        TokenStream::from(err.to_compile_error())
    }
}

fn process_casper_message_for_struct(
    item_struct: &ItemStruct,
    struct_meta: StructMeta,
) -> TokenStream {
    let struct_name = &item_struct.ident;

    let crate_path = match &struct_meta.path {
        Some(path) => quote! { #path },
        None => quote! { casper_contract_sdk },
    };

    let borsh_path = {
        let crate_path_str = match &struct_meta.path {
            Some(path) => path.to_token_stream().to_string(),
            None => "casper_contract_sdk".to_string(),
        };
        syn::LitStr::new(
            &format!("{}::serializers::borsh", crate_path_str),
            Span::call_site(),
        )
    };

    let maybe_derive_abi = get_maybe_derive_abi(crate_path.clone());
    let maybe_entrypoint_defs;

    let topic = match struct_meta.message {
        Some(Override::Inherit) => quote! { stringify!(#struct_name) },
        Some(Override::Explicit(message_meta)) => {
            let MessageMeta { topic } = message_meta;
            quote! { #topic }
        }
        None => {
            let err =
                syn::Error::new_spanned(&item_struct.ident, "Message attribute requires a topic");
            return TokenStream::from(err.to_compile_error());
        }
    };

    #[cfg(feature = "__abi_generator")]
    {
        maybe_entrypoint_defs = quote! {
            #[cfg(not(target_arch = "wasm32"))]
            const _: () = {
                #[casper_contract_sdk::linkme::distributed_slice(casper_contract_sdk::abi::collector::ABI_ITEMS)]
                #[linkme(crate = casper_contract_sdk::linkme)]
                pub static ABI_ITEM: casper_contract_sdk::abi::collector::AbiItem = casper_contract_sdk::abi::collector::AbiItem::Message(casper_contract_sdk::abi::collector::AbiMessage {
                    topic: || #topic,
                    decl: casper_contract_sdk::abi::collector::AbiType {
                        type_name: core::any::type_name::<#struct_name>,
                        type_id: casper_contract_sdk::common::type_uid::of::<#struct_name>(),
                        cl_type: <#struct_name as casper_contract_sdk::compat::types::CLTyped>::cl_type,
                        visit_abi_types: |visitor| {
                            casper_contract_sdk::abi::visit_types_recursively::<#struct_name>(visitor);
                        },
                    }
                });
            };
        }
    }
    #[cfg(not(feature = "__abi_generator"))]
    {
        maybe_entrypoint_defs = quote! {};
    }

    quote! {
        #[derive(#crate_path::serializers::borsh::BorshSerialize, #crate_path::macros::TypeUid)]
        #[type_uid(crate = #crate_path::common::type_uid)]
        #[borsh(crate = #borsh_path)]
        #maybe_derive_abi
        #item_struct

        impl #crate_path::Message for #struct_name {
            const TOPIC: &'static str = #topic;

            #[inline]
            fn payload(&self) -> casper_contract_sdk::prelude::vec::Vec<u8> {
                #crate_path::serializers::borsh::to_vec(self).unwrap()
            }
        }

        impl #crate_path::compat::types::CLTyped for #struct_name {
            fn cl_type() -> #crate_path::compat::types::CLType {
                #crate_path::compat::types::CLType::Any
            }
        }

        #maybe_entrypoint_defs

    }
    .into()
}

fn generate_export_function(func: &ItemFn) -> TokenStream {
    let func_name = &func.sig.ident;
    let mut arg_names = Vec::new();
    let mut arg_types = Vec::new();
    for input in &func.sig.inputs {
        let (name, ty) = match input {
            syn::FnArg::Receiver(receiver) => {
                todo!("{receiver:?}")
            }
            syn::FnArg::Typed(typed) => match typed.pat.as_ref() {
                syn::Pat::Ident(ident) => (&ident.ident, &typed.ty),
                _ => todo!("export: other typed variant"),
            },
        };
        arg_names.push(name);
        arg_types.push(ty);
    }

    let ret = match &func.sig.output {
        syn::ReturnType::Default => quote! { () },
        syn::ReturnType::Type(_, ty) => quote! { #ty },
    };

    let _ctor_name = format_ident!("{func_name}_ctor");

    let exported_func_name = format_ident!("__casper_export_{func_name}");
    quote! {
        #[cfg(target_arch = "wasm32")]
        #[export_name = stringify!(#func_name)]
        #[no_mangle]
        pub extern "C" fn #exported_func_name() {
            casper_contract_sdk::set_panic_hook();
            #func

            #[derive(casper_contract_sdk::serializers::borsh::BorshDeserialize)]
            #[borsh(crate = "casper_contract_sdk::serializers::borsh")]
            struct Arguments {
                #(#arg_names: #arg_types,)*
            }
            let input = casper_contract_sdk::prelude::casper::copy_input();
            let args: Arguments = casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap();
            let _ret = #func_name(#(args.#arg_names,)*);
        }

        #[cfg(not(target_arch = "wasm32"))]
        pub fn #exported_func_name() {
            #func

            #[derive(casper_contract_sdk::serializers::borsh::BorshDeserialize)]
            #[borsh(crate = "casper_contract_sdk::serializers::borsh")]
            struct Arguments {
                #(#arg_names: #arg_types,)*
            }
            let input = casper_contract_sdk::prelude::casper::copy_input();
            let args: Arguments = casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap();
            let _ret = #func_name(#(args.#arg_names,)*);
        }

        #[cfg(not(target_arch = "wasm32"))]
        #func

        #[cfg(not(target_arch = "wasm32"))]
        const _: () = {
            const NAME: &str = stringify!(#func_name);
            const EXPORT_NAME: &str = NAME;

            #[casper_contract_sdk::linkme::distributed_slice(casper_contract_sdk::abi::collector::ABI_ITEMS)]
            #[linkme(crate = casper_contract_sdk::linkme)]
            pub static EXPORTS: casper_contract_sdk::abi::collector::AbiItem = casper_contract_sdk::abi::collector::AbiItem::EntryPoint(casper_contract_sdk::abi::collector::AbiEntryPoint {
                name: NAME,
                export_name: EXPORT_NAME,
                receiver: casper_contract_sdk::abi::collector::AbiReceiver::NoReceiver,
                is_constructor: false, // todo
                is_payable: false, // todo
                params: &[
                    #(
                    casper_contract_sdk::abi::collector::AbiParam {
                        name: stringify!(#arg_names),
                        decl: casper_contract_sdk::abi::collector::AbiType {
                            type_name: core::any::type_name::<#arg_types>,
                            type_id: casper_contract_sdk::common::type_uid::of::<#arg_types>(),
                            cl_type: <#arg_types as casper_contract_sdk::compat::types::CLTyped>::cl_type,
                            visit_abi_types: |visitor| {
                                casper_contract_sdk::abi::visit_types_recursively::<#arg_types>(visitor);
                            },
                        }
                    },
                    )*
                ],
                abi_convention: casper_contract_sdk::serializers::AbiConvention::Positional, // todo
                result_decl: {
                    casper_contract_sdk::abi::collector::AbiType {
                        type_name: core::any::type_name::<#ret>,
                        type_id: casper_contract_sdk::common::type_uid::of::<#ret>(),
                        cl_type: <#ret as casper_contract_sdk::compat::types::CLTyped>::cl_type,
                        visit_abi_types: |visitor| {
                            casper_contract_sdk::abi::visit_types_recursively::<#ret>(visitor);
                        },
                    }
                },
                kind: casper_contract_sdk::abi::collector::AbiKind::Function,
                location: casper_contract_sdk::abi::collector::AbiLocation {
                    file: file!(),
                    line: line!(),
                    col: column!(),
                },
                fptr: || -> () { #exported_func_name(); },
            });
        };
    }.into()
}

fn generate_impl_for_contract(mut entry_points: ItemImpl) -> TokenStream {
    // #[cfg(feature = "__abi_generator")]
    // let mut populate_definitions_linkme = Vec::new();
    let impl_trait = match entry_points.trait_.as_ref() {
        Some((None, path, _for)) => Some(path),
        Some((Some(_not), _path, _for)) => {
            panic!("Exclamation mark not supported");
        }
        None => None,
    };
    let struct_name = match entry_points.self_ty.as_ref() {
        Type::Path(ref path) => &path.path,

        other => todo!("Unsupported {other:?}"),
    };
    let defs = vec![quote! {}]; // TODO: Dummy element which may not be necessary but is used for expansion later
    let mut names = Vec::new();
    let mut extern_entry_points = Vec::new();
    let _abi_generator_entry_points = [quote! {}]; // TODO: Dummy element which may not be necessary but is used for expansion later
    let mut manifest_entry_point_enum_variants = Vec::new();
    let mut manifest_entry_point_enum_match_name = Vec::new();
    let mut manifest_entry_point_input_data = Vec::new();
    let mut extra_code = Vec::new();

    // let mut collected_abi_types = Vec::new();

    for entry_point in &mut entry_points.items {
        let method_attribute;

        match entry_point {
            syn::ImplItem::Const(_) => todo!("Const"),
            syn::ImplItem::Fn(ref mut func) => {
                let vis = &func.vis;
                match vis {
                    syn::Visibility::Public(_) => {}
                    syn::Visibility::Inherited => {
                        // As the doc says this "usually means private"
                        continue;
                    }
                    syn::Visibility::Restricted(_restricted) => {}
                }

                // func.sig.re
                let never_returns = match &func.sig.output {
                    syn::ReturnType::Default => false,
                    syn::ReturnType::Type(_, ty) => matches!(ty.as_ref(), Type::Never(_)),
                };

                if never_returns {
                    let err = syn::Error::new_spanned(
                        &func.sig.output,
                        "Diverging functions (`-> !`) are not supported as entry points",
                    );
                    return TokenStream::from(err.to_compile_error());
                }

                method_attribute = MethodAttribute::from_attributes(&func.attrs).unwrap();

                let is_payable = if method_attribute.payable {
                    quote! { true }
                } else {
                    quote! { false }
                };
                let is_constructor = if method_attribute.constructor {
                    quote! { true }
                } else {
                    quote! { false }
                };

                func.attrs.clear();

                if method_attribute.private {
                    continue;
                }

                let func_name = func.sig.ident.clone();
                if func_name.to_string().starts_with("__casper_") {
                    return TokenStream::from(
                        syn::Error::new(
                            Span::call_site(),
                            "Function names starting with '__casper_' are reserved",
                        )
                        .to_compile_error(),
                    );
                }

                let export_name = format_ident!("{}", &func_name);
                let inner_func_name = format_ident!("__casper_export_inner_{}", &func_name);

                names.push(func_name.clone());

                let arg_names_and_types = func
                    .sig
                    .inputs
                    .iter()
                    .filter_map(|arg| match arg {
                        syn::FnArg::Receiver(_) => None,
                        syn::FnArg::Typed(typed) => match typed.pat.as_ref() {
                            syn::Pat::Ident(ident) => Some((&ident.ident, &typed.ty)),
                            _ => todo!(),
                        },
                    })
                    .collect::<Vec<_>>();

                let arg_names: Vec<_> =
                    arg_names_and_types.iter().map(|(name, _ty)| name).collect();
                let arg_types: Vec<_> = arg_names_and_types.iter().map(|(_name, ty)| ty).collect();
                let arg_attrs: Vec<_> = arg_names_and_types
                    .iter()
                    .map(|(name, ty)| quote! { #name: #ty })
                    .collect();

                let (receiver_is_ref, receiver_is_mut, receiver_exists) =
                    match func.sig.inputs.first() {
                        Some(syn::FnArg::Receiver(receiver)) => (
                            receiver.reference.is_some(),
                            receiver.mutability.is_some(),
                            true,
                        ),
                        _ => (false, false, false),
                    };

                let call_data_return_lifetime = if method_attribute.constructor {
                    quote! {
                        #struct_name
                    }
                } else {
                    generate_call_data_return(&func.sig.output)
                };
                let _func_sig_output = match &func.sig.output {
                    syn::ReturnType::Default => {
                        quote! { () }
                    }
                    syn::ReturnType::Type(_, ty) => {
                        quote! { #ty }
                    }
                };

                let resolve_abi_convention = match method_attribute.abi_convention {
                    Some(convention) => {
                        // If method specifies a convention, then use it
                        quote! { #convention }
                    }
                    None => quote! {
                        // If method does not specify a convention, resolve to the convention specified at the struct level.
                        <#struct_name as casper_contract_sdk::serializers::AbiConfig>::DEFAULT_ABI_CONVENTION
                    },
                };

                let ret_ty;

                let handle_ret = if never_returns {
                    ret_ty = Some(quote! { ! });
                    None
                } else {
                    match func.sig.output {
                        syn::ReturnType::Default => {
                            // Do not call casper_return if there is no return value

                            ret_ty = Some(quote! { () });

                            Some(quote! {
                                match #resolve_abi_convention {
                                    casper_contract_sdk::serializers::AbiConvention::Positional => {
                                        // Do nothing as lack of ret is synonymous with returning empty bytes (unit serializes to empty buffer)
                                    }
                                    casper_contract_sdk::serializers::AbiConvention::Named => {
                                        // For a named ABI convention we'd always ret with the bytes of unit CLValue.
                                        let ret_bytes = casper_contract_sdk::serializers::borsh::to_vec(&casper_contract_sdk::compat::types::CLValue::UNIT).expect("Failed to serialize return CLValue");
                                        casper_contract_sdk::casper::ret(flags, Some(&ret_bytes))
                                    }
                                }
                            })
                        }
                        _ if method_attribute.constructor => {
                            // Constructor does not return serialized state but is expected to save
                            // state, or explicitly revert.
                            // TODO: Add support for Result<Self, Error> and rollback_on_error if
                            // possible.

                            ret_ty = Some(quote! { #struct_name });

                            Some(quote! {
                                let _ = flags; // hide the warning
                            })
                        }
                        syn::ReturnType::Type(_rarrow, ref ty) => {
                            // There is a return value so call casper_return.
                            // ret_ty =/
                            ret_ty = Some(quote! { #ty });

                            Some(quote! {
                                let ret_bytes = match #resolve_abi_convention {
                                    casper_contract_sdk::serializers::AbiConvention::Positional => {
                                        casper_contract_sdk::serializers::borsh::to_vec(&_ret).expect("Failed to serialize return value")
                                    }
                                    casper_contract_sdk::serializers::AbiConvention::Named => {
                                        let ret_clvalue = casper_contract_sdk::compat::types::CLValue::from_t(&_ret).expect("Failed to convert return value to CLValue");

                                        casper_contract_sdk::serializers::borsh::to_vec(&ret_clvalue).expect("Failed to serialize return CLValue")
                                    }
                                };
                                casper_contract_sdk::casper::ret(flags, Some(&ret_bytes))
                            })
                        }
                    }
                };

                assert_eq!(arg_names.len(), arg_types.len());

                let mut prelude = Vec::new();

                prelude.push(quote! {
                    #[derive(casper_contract_sdk::serializers::borsh::BorshDeserialize)]
                    #[borsh(crate = "casper_contract_sdk::serializers::borsh")]
                    struct Arguments {
                        #(#arg_attrs,)*
                    }


                    let input = casper_contract_sdk::prelude::casper::copy_input();
                    let resolved_abi_convention = #resolve_abi_convention;
                    let args: Arguments = {
                        match resolved_abi_convention {
                            casper_contract_sdk::serializers::AbiConvention::Positional => {
                                casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap()
                            }
                            casper_contract_sdk::serializers::AbiConvention::Named => {
                                let runtime_args: casper_contract_sdk::compat::types::RuntimeArgs =
                                    casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap();

                                #(
                                    let #arg_names: #arg_types = {
                                        let cl_value = runtime_args.get(stringify!(#arg_names)).unwrap_or_else(|| panic!(concat!("Failed to get named argument \"", stringify!(#arg_names), "\"")));
                                        cl_value.to_t::<#arg_types>().unwrap_or_else(|error| {
                                            panic!(concat!("Failed to convert named argument \"", stringify!(#arg_names), "\": {}"), error)
                                        })
                                    };
                                )*

                                Arguments {
                                    #(
                                        #arg_names,
                                    )*
                                }
                            }
                        }
                    }
                });

                if method_attribute.constructor {
                    prelude.push(quote! {
                        if casper_contract_sdk::casper::is_contract_state_initialized().unwrap() {
                            panic!("State of the contract is already present; unable to proceed with the constructor");
                        }
                    });
                }

                if !method_attribute.payable {
                    let panic_msg = format!(
                        r#"Entry point "{func_name}" is not payable and does not accept tokens"#
                    );
                    prelude.push(quote! {
                        if casper_contract_sdk::casper::transferred_value() != 0 {
                            // TODO: Be precise and unambigious about the error
                            panic!(#panic_msg);
                        }
                    });
                }

                let handle_err = if !never_returns && method_attribute.rollback_on_error {
                    if let syn::ReturnType::Default = func.sig.output {
                        panic!("Cannot revert on error if there is no return value");
                    }

                    quote! {
                        let _ret: &Result<_, _> = &_ret;
                        if _ret.is_err() {
                            flags |= casper_contract_sdk::common::flags::ReturnFlags::ROLLBACK;
                        }

                    }
                } else {
                    quote! {}
                };

                let handle_call = if receiver_exists {
                    if receiver_is_ref {
                        quote! {
                            let mut instance: #struct_name = {
                                use casper_contract_sdk::FieldStateAccess;
                                #struct_name::read_state_from_fields().unwrap()
                            };
                            let _ret = instance.#func_name(#(args.#arg_names,)*);
                        }
                    } else {
                        quote! {
                            let _ret = {
                                use casper_contract_sdk::FieldStateAccess;
                                #struct_name::read_state_from_fields().unwrap().#func_name(#(args.#arg_names,)*)
                            };
                        }
                    }
                } else if method_attribute.constructor {
                    quote! {
                        let _ret = <#struct_name>::#func_name(#(args.#arg_names,)*);
                    }
                } else {
                    quote! {
                        let _ret = <#struct_name>::#func_name(#(args.#arg_names,)*);
                    }
                };

                let extern_func_name = format_ident!("__casper_export_{func_name}");

                let persist_after_call_tokens = if method_attribute.constructor {
                    quote! {
                        {
                            use casper_contract_sdk::FieldStateAccess;
                            casper_contract_sdk::casper::mark_contract_initialization_state(true).expect("Failed to mark contract state as initialized");
                            let _ = _ret.write_state_to_fields().unwrap();
                        }
                    }
                } else if receiver_is_ref && receiver_is_mut {
                    quote! {
                        {
                            use casper_contract_sdk::FieldStateAccess;
                            let _ = instance.write_state_to_fields().unwrap();
                        }
                    }
                } else {
                    quote! {}
                };

                let abi_receiver = match parse_abi_receiver(&func.sig) {
                    Ok(v) => v,
                    Err(e) => {
                        return e;
                    }
                };

                extern_entry_points.push(quote! {

                    #[export_name = stringify!(#export_name)]
                    #[cfg(target_arch = "wasm32")]
                    #vis extern "C" fn #extern_func_name() {
                        #inner_func_name();
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    #vis fn #extern_func_name() {
                        #inner_func_name();
                    }

                    fn #inner_func_name() {
                        // Set panic hook (assumes std is enabled etc.)
                        #[cfg(target_arch = "wasm32")]
                        {
                            casper_contract_sdk::set_panic_hook();
                        }

                        #(#prelude;)*

                        let mut flags = casper_contract_sdk::common::flags::ReturnFlags::empty();

                        #handle_call;

                        #handle_err;
                        #persist_after_call_tokens
                        #handle_ret;
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    const _: () = {
                        #[casper_contract_sdk::linkme::distributed_slice(casper_contract_sdk::abi::collector::ABI_ITEMS)]
                        #[linkme(crate = casper_contract_sdk::linkme)]
                        pub static EXPORTS: casper_contract_sdk::abi::collector::AbiItem = casper_contract_sdk::abi::collector::AbiItem::EntryPoint(casper_contract_sdk::abi::collector::AbiEntryPoint {
                            name: stringify!(#export_name),
                            export_name: stringify!(#export_name),
                            receiver: #abi_receiver,
                            is_constructor: #is_constructor,
                            is_payable: #is_payable,
                            params: &[
                                #(
                                    casper_contract_sdk::abi::collector::AbiParam {
                                        name: stringify!(#arg_names),
                                        decl: casper_contract_sdk::abi::collector::AbiType {
                                            type_name: core::any::type_name::<#arg_types>,
                                            type_id: casper_contract_sdk::common::type_uid::of::<#arg_types>(),
                                            cl_type: <#arg_types as casper_contract_sdk::compat::types::CLTyped>::cl_type,
                                            visit_abi_types: |visitor| {
                                                casper_contract_sdk::abi::visit_types_recursively::<#arg_types>(visitor);
                                            },
                                        }
                                    },
                                )*
                            ],
                            abi_convention: #resolve_abi_convention,
                            result_decl: casper_contract_sdk::abi::collector::AbiType {
                                type_name: core::any::type_name::<#ret_ty>,
                                type_id: casper_contract_sdk::common::type_uid::of::<#ret_ty>(),
                                cl_type: <#ret_ty as casper_contract_sdk::compat::types::CLTyped>::cl_type,
                                visit_abi_types: |visitor| {
                                    casper_contract_sdk::abi::visit_types_recursively::<#ret_ty>(visitor);
                                },
                            },
                            kind: casper_contract_sdk::abi::collector::AbiKind::SmartContract { struct_name: stringify!(#struct_name) },
                            location: casper_contract_sdk::abi::collector::AbiLocation {
                                file: file!(),
                                line: line!(),
                                col: column!(),
                            },
                            fptr: || -> () { #extern_func_name(); },
                        });
                    };
                });

                manifest_entry_point_enum_variants.push(quote! {
                    #func_name {
                        #(#arg_names: #arg_types,)*
                    }
                });

                manifest_entry_point_enum_match_name.push(quote! {
                    #func_name
                });

                manifest_entry_point_input_data.push(quote! {
                    Self::#func_name { #(#arg_names,)* } => {
                        let into_tuple = (#(#arg_names,)*);
                        into_tuple.serialize(writer)
                    }
                });

                match entry_points.self_ty.as_ref() {
                    Type::Path(ref path) => {
                        let ident = syn::Ident::new(
                            &format!("{}_{}", path.path.get_ident().unwrap(), func_name),
                            Span::call_site(),
                        );

                        let input_data_content = if arg_names.is_empty() {
                            quote! {
                                None
                            }
                        } else {
                            quote! {
                                Some(casper_contract_sdk::serializers::borsh::to_vec(&self).expect("Serialization to succeed"))
                            }
                        };

                        let self_ty =
                            if method_attribute.constructor || method_attribute.ignore_state {
                                None
                            } else {
                                Some(quote! {
                                   &self,
                                })
                            };

                        extra_code.push(quote! {
                            pub fn #func_name<'a>(#self_ty #(#arg_names: #arg_types,)*) -> impl casper_contract_sdk::ToCallData<Return<'a> = #call_data_return_lifetime> {
                                #[derive(casper_contract_sdk::serializers::borsh::BorshSerialize, PartialEq, Debug)]
                                #[borsh(crate = "casper_contract_sdk::serializers::borsh")]
                                struct #ident {
                                    #(#arg_names: #arg_types,)*
                                }

                                impl casper_contract_sdk::ToCallData for #ident {
                                    type Return<'a> = #call_data_return_lifetime;

                                    fn entry_point(&self) -> &str { stringify!(#func_name) }

                                    fn input_data(&self) -> Option<casper_contract_sdk::serializers::borsh::__private::maybestd::vec::Vec<u8>> {
                                        #input_data_content
                                    }
                                }

                                #ident {
                                    #(#arg_names,)*
                                }
                            }
                        });
                    }

                    _ => todo!("Different self_ty currently unsupported"),
                }
            }
            syn::ImplItem::Type(_) => todo!(),
            syn::ImplItem::Macro(_) => todo!(),
            syn::ImplItem::Verbatim(_) => todo!(),
            _ => todo!(),
        }
    }
    // let entry_points_len = entry_points.len();
    let st_name = struct_name.get_ident().unwrap();
    let handle_manifest = match impl_trait {
        Some(_path) => {
            // Do not generate a manifest if we're implementing a trait.
            // The expectation is that you list the traits below under
            // #[derive(Contract)] and the rest is handled by a macro
            None
        }
        None => Some(quote! {

            #[doc(hidden)]
            impl #struct_name {
                #(#defs)*
            }

            #(#extern_entry_points)*

        }),
    };
    let ref_struct_name = format_ident!("{st_name}Ref");

    quote! {
        #entry_points

        #handle_manifest

        impl #ref_struct_name {
            #(#extra_code)*
        }
    }
    .into()
}

fn generate_impl_trait_for_contract(
    entry_points: &ItemImpl,
    trait_path: &syn::Path,
    impl_meta: ImplTraitForContractMeta,
) -> TokenStream {
    let self_ty = match entry_points.self_ty.as_ref() {
        Type::Path(ref path) => &path.path,
        other => todo!("Unsupported {other:?}"),
    };
    let self_ty = quote! { #self_ty };
    let mut code = Vec::new();

    let trait_name = trait_path
        .segments
        .last()
        .expect("Expected non-empty path")
        .ident
        .clone();

    let path_to_macro = match &impl_meta.path {
        Some(path) => quote! { #path },
        None => {
            quote! { self }
        }
    };

    let path_to_crate: proc_macro2::TokenStream = match &impl_meta.path {
        Some(path) => {
            let crate_name = path
                .segments
                .first()
                .expect("Expected non-empty path")
                .ident
                .clone();

            if crate_name == "crate" {
                // This is local, can't refer by absolute path
                quote! { #path }
            } else {
                quote! { #crate_name }
            }
        }
        None => {
            quote! { self }
        }
    };

    let macro_name = format_ident!("enumerate_{trait_name}_symbols");

    let visitor = if impl_meta.compile_as_dependency {
        quote! {
            const _: () = {
                macro_rules! visitor {
                    ( $( @exportas $export_name:ident, @is_constructor $is_constructor:ident, @is_payable $is_payable:ident, @abi_convention $abi_convention:expr, @receiver $receiver:expr, $vis:vis fn $name:ident( $($arg:ident: $argty:ty $(,)*)* ) -> $ret:ty ; ) * ) => {
                        $(
                            const _: () = {
                                $vis extern "C" fn $name() {
                                    #path_to_macro::$name::<#self_ty>();
                                }

                               #[cfg(not(target_arch = "wasm32"))]
                                const _: () = {
                                    const NAME: &str = stringify!($name);
                                    const EXPORT_NAME: &str = stringify!($export_name);

                                    #[casper_contract_sdk::linkme::distributed_slice(casper_contract_sdk::abi::collector::ABI_ITEMS)]
                                    #[linkme(crate = casper_contract_sdk::linkme)]
                                    pub static EXPORTS: casper_contract_sdk::abi::collector::AbiItem = casper_contract_sdk::abi::collector::AbiItem::EntryPoint(casper_contract_sdk::abi::collector::AbiEntryPoint {
                                        name: NAME,
                                        export_name: EXPORT_NAME,
                                        receiver: $receiver,
                                        is_constructor: $is_constructor,
                                        is_payable: $is_payable,
                                        params: &[
                                            $(
                                                casper_contract_sdk::abi::collector::AbiParam {
                                                    name: stringify!($arg),
                                                    decl: casper_contract_sdk::abi::collector::AbiType {
                                                        type_name: core::any::type_name::<$argty>,
                                                        type_id: casper_contract_sdk::common::type_uid::of::<$argty>(),
                                                        cl_type: <$argty as casper_contract_sdk::compat::types::CLTyped>::cl_type,
                                                        visit_abi_types: |visitor| {
                                                            casper_contract_sdk::abi::visit_types_recursively::<$argty>(visitor);
                                                        },
                                                    }
                                                },
                                            )*
                                        ],
                                        abi_convention: $abi_convention,
                                        result_decl: {
                                            use #path_to_crate::*;
                                            casper_contract_sdk::abi::collector::AbiType {
                                                type_name: core::any::type_name::<$ret>,
                                                type_id: casper_contract_sdk::common::type_uid::of::<$ret>(),
                                                cl_type: || { <$ret as casper_contract_sdk::compat::types::CLTyped>::cl_type() },
                                                visit_abi_types: |visitor| {
                                                    casper_contract_sdk::abi::visit_types_recursively::<$ret>(visitor);
                                                },
                                            }
                                        },
                                        kind: casper_contract_sdk::abi::collector::AbiKind::TraitImpl { struct_name: NAME, trait_name: stringify!(#trait_name)},
                                        location: casper_contract_sdk::abi::collector::AbiLocation {
                                            file: file!(),
                                            line: line!(),
                                            col: column!(),
                                        },
                                        fptr: || -> () { $name(); },
                                    });
                                };
                            };
                        )*
                    }
                }

                #path_to_crate::#macro_name!(visitor);
            };
        }
    } else {
        quote! {
            const _: () = {
                macro_rules! visitor {
                    ( $( @exportas $export_name:ident, @is_constructor $is_constructor:ident, @is_payable $is_payable:ident, @abi_convention $abi_convention:expr, @receiver $receiver:path, $vis:vis fn $name:ident( $($arg:ident: $argty:ty $(,)*)* ) -> $ret:ty ; ) * ) => {
                        $(
                            const _: () = {
                                #[export_name = stringify!($export_name)]
                                $vis extern "C" fn $name() {
                                    #path_to_macro::$name::<#self_ty>();
                                }

                               #[cfg(not(target_arch = "wasm32"))]
                                const _: () = {
                                    const NAME: &str = stringify!($name);
                                    const EXPORT_NAME: &str = stringify!($export_name);

                                    #[casper_contract_sdk::linkme::distributed_slice(casper_contract_sdk::abi::collector::ABI_ITEMS)]
                                    #[linkme(crate = casper_contract_sdk::linkme)]
                                    pub static EXPORTS: casper_contract_sdk::abi::collector::AbiItem = casper_contract_sdk::abi::collector::AbiItem::EntryPoint(casper_contract_sdk::abi::collector::AbiEntryPoint {
                                        name: NAME,
                                        export_name: EXPORT_NAME,

                                        receiver: $receiver,
                                        is_constructor: $is_constructor,
                                        is_payable: $is_payable,
                                        params: &[
                                            $(
                                                casper_contract_sdk::abi::collector::AbiParam {
                                                    name: stringify!($arg),
                                                    decl: casper_contract_sdk::abi::collector::AbiType {
                                                        type_name: core::any::type_name::<$argty>,
                                                        type_id: casper_contract_sdk::common::type_uid::of::<$argty>(),
                                                        cl_type: <$argty as casper_contract_sdk::compat::types::CLTyped>::cl_type,
                                                        visit_abi_types: |visitor| {
                                                            casper_contract_sdk::abi::visit_types_recursively::<$argty>(visitor);
                                                        },
                                                    }
                                                },
                                            )*
                                        ],
                                        abi_convention: $abi_convention,
                                        result_decl: {
                                            casper_contract_sdk::abi::collector::AbiType {
                                                type_name: core::any::type_name::<$ret>,
                                                type_id: casper_contract_sdk::common::type_uid::of::<$ret>(),
                                                cl_type: || { <$ret as casper_contract_sdk::compat::types::CLTyped>::cl_type() },
                                                visit_abi_types: |visitor| {
                                                    casper_contract_sdk::abi::visit_types_recursively::<$ret>(visitor);
                                                },
                                            }
                                        },
                                        kind: casper_contract_sdk::abi::collector::AbiKind::TraitImpl { struct_name: NAME, trait_name: stringify!(#trait_name)},
                                        location: casper_contract_sdk::abi::collector::AbiLocation {
                                            file: file!(),
                                            line: line!(),
                                            col: column!(),
                                        },
                                        fptr: || -> () { $name(); },
                                    });
                                };
                            };
                        )*
                    }
                }

                #path_to_crate::#macro_name!(visitor);
            };
        }
    };

    code.push(visitor);

    let ext_trait = format_ident!("{}Ext", trait_path.require_ident().unwrap());

    let ref_name = format_ident!("{self_ty}Ref");

    code.push(quote! {
        impl #ext_trait for #ref_name {}
    });

    quote! {
        #entry_points

        #(#code)*
    }
    .into()
}

fn casper_trait_definition(mut item_trait: ItemTrait, trait_meta: TraitMeta) -> TokenStream {
    let crate_path = match &trait_meta.path {
        Some(path) => quote! { #path },
        None => quote! { casper_contract_sdk },
    };

    let borsh_path = {
        let crate_path_str = match &trait_meta.path {
            Some(path) => path.to_token_stream().to_string(),
            None => "casper_contract_sdk".to_string(),
        };
        syn::LitStr::new(
            &format!("{}::serializers::borsh", crate_path_str),
            Span::call_site(),
        )
    };

    let trait_name = &item_trait.ident;

    let vis = &item_trait.vis;
    let mut dispatch_functions = Vec::new();
    let mut extra_code = Vec::new();

    let mut macro_symbols = Vec::new();
    for entry_point in &mut item_trait.items {
        match entry_point {
            syn::TraitItem::Const(_) => todo!("Const"),
            syn::TraitItem::Fn(func) => {
                let method_attribute = MethodAttribute::from_attributes(&func.attrs).unwrap();
                func.attrs.clear();

                if method_attribute.private {
                    continue;
                }

                let func_name = func.sig.ident.clone();
                let func_name_str = func_name.to_string();

                if func_name.to_string().starts_with("__casper_") {
                    return TokenStream::from(
                        syn::Error::new(
                            Span::call_site(),
                            "Function names starting with '__casper_' are reserved",
                        )
                        .to_compile_error(),
                    );
                }

                let export_name = format_ident!("{trait_name}_{func_name_str}");

                let call_data_return_lifetime = generate_call_data_return(&func.sig.output);

                let dispatch_func_name = format_ident!("{func_name}");

                let arg_names_and_types = func
                    .sig
                    .inputs
                    .iter()
                    .filter_map(|arg| match arg {
                        syn::FnArg::Receiver(_) => None,
                        syn::FnArg::Typed(typed) => match typed.pat.as_ref() {
                            syn::Pat::Ident(ident) => Some((&ident.ident, &typed.ty)),
                            _ => todo!(),
                        },
                    })
                    .collect::<Vec<_>>();

                let arg_names: Vec<_> =
                    arg_names_and_types.iter().map(|(name, _ty)| name).collect();
                let arg_types: Vec<_> = arg_names_and_types.iter().map(|(_name, ty)| ty).collect();
                // let mut arg_pairs: Vec
                let args_attrs: Vec<_> = arg_names_and_types
                    .iter()
                    .map(|(name, ty)| {
                        quote! {
                            #name: #ty
                        }
                    })
                    .collect();

                let trait_ref = format_ident!("{}Ref", trait_name);
                let resolve_abi_convention = match method_attribute.abi_convention.as_ref() {
                    Some(ref convention) => {
                        // If method specifies a convention, then use it
                        quote! { #convention }
                    }
                    None => quote! {
                        // If method does not specify a convention, resolve to the convention specified at the struct level.
                        <#trait_ref as casper_contract_sdk::serializers::AbiConfig>::DEFAULT_ABI_CONVENTION
                    },
                };

                let abi_convention = match method_attribute.abi_convention.as_ref() {
                    Some(ref abi_convention) => quote! { #abi_convention },
                    None => trait_meta
                        .abi_convention
                        .as_ref()
                        .map(|abi_convention| quote! { #abi_convention })
                        .unwrap_or(
                            quote! { casper_contract_sdk::serializers::AbiConvention::Positional },
                        ),
                };

                let never_returns = match &func.sig.output {
                    syn::ReturnType::Default => false,
                    syn::ReturnType::Type(_, ty) => matches!(ty.as_ref(), Type::Never(_)),
                };

                let ret_ty;

                let handle_ret = if never_returns {
                    ret_ty = quote! { ! };
                    None
                } else {
                    match &func.sig.output {
                        syn::ReturnType::Default => {
                            // Do not call casper_return if there is no return value

                            ret_ty = quote! { () };

                            Some(quote! {
                                match #resolve_abi_convention {
                                    casper_contract_sdk::serializers::AbiConvention::Positional => {
                                        // Do nothing as lack of ret is synonymous with returning empty bytes (unit serializes to empty buffer)
                                    }
                                    casper_contract_sdk::serializers::AbiConvention::Named => {
                                        // For a named ABI convention we'd always ret with the bytes of unit CLValue.
                                        let ret_bytes = casper_contract_sdk::serializers::borsh::to_vec(&casper_contract_sdk::compat::types::CLValue::UNIT).expect("Failed to serialize return CLValue");
                                        casper_contract_sdk::casper::ret(flags, Some(&ret_bytes))
                                    }
                                }
                            })
                        }
                        _ if method_attribute.constructor => {
                            // Constructor does not return serialized state but is expected to save
                            // state, or explicitly revert.
                            // TODO: Add support for Result<Self, Error> and rollback_on_error if
                            // possible.
                            ret_ty = quote! { Self };

                            Some(quote! {
                                let _ = flags; // hide the warning
                            })
                        }
                        syn::ReturnType::Type(_, ty) => {
                            ret_ty = quote! { #ty };

                            // There is a return value so call casper_return.
                            Some(quote! {
                                let ret_bytes = match #resolve_abi_convention {
                                    casper_contract_sdk::serializers::AbiConvention::Positional => {
                                        casper_contract_sdk::serializers::borsh::to_vec(&_ret).expect("Failed to serialize return value")
                                    }
                                    casper_contract_sdk::serializers::AbiConvention::Named => {
                                        let ret_clvalue = casper_contract_sdk::compat::types::CLValue::from_t(&_ret).expect("Failed to convert return value to CLValue");

                                        casper_contract_sdk::serializers::borsh::to_vec(&ret_clvalue).expect("Failed to serialize return CLValue")
                                    }
                                };
                                casper_contract_sdk::casper::ret(flags, Some(&ret_bytes))
                            })
                        }
                    }
                };

                let handle_dispatch = match func.sig.inputs.first() {
                    Some(syn::FnArg::Receiver(receiver)) => {
                        assert!(
                            !method_attribute.private,
                            "can't make dispatcher for private method"
                        );
                        let is_by_ref = receiver.reference.is_some();
                        let is_mut = receiver.mutability.is_some();
                        quote! {
                            #[cfg(target_arch = "wasm32")]
                            #[no_mangle]
                            #vis extern "C" fn #dispatch_func_name<T>()
                            where
                                T: #trait_name
                                    + #crate_path::serializers::borsh::BorshDeserialize
                                    + #crate_path::serializers::borsh::BorshSerialize
                                    + #crate_path::FieldStateAccess
                                    + Default
                            {
                                use casper_contract_sdk::FieldStateAccess;

                                #[derive(#crate_path::serializers::borsh::BorshDeserialize, Debug)]
                                #[borsh(crate = #borsh_path)]
                                struct Arguments {
                                    #(#args_attrs,)*
                                }

                                let mut flags = #crate_path::common::flags::ReturnFlags::empty();
                                let mut instance: T = T::read_state_from_fields().unwrap();
                                let input = #crate_path::prelude::casper::copy_input();
                                let args: Arguments = {
                                    match #resolve_abi_convention {
                                        casper_contract_sdk::serializers::AbiConvention::Positional => {
                                            casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap()
                                        }
                                        casper_contract_sdk::serializers::AbiConvention::Named => {
                                            let runtime_args: casper_contract_sdk::compat::types::RuntimeArgs =
                                                casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap();
                                            #(
                                                let #arg_names: #arg_types = {
                                                    let cl_value = runtime_args.get(stringify!(#arg_names)).unwrap_or_else(|| panic!(concat!("Failed to get named argument \"", stringify!(#arg_names), "\"")));
                                                    cl_value.to_t::<#arg_types>().unwrap_or_else(|error| {
                                                        panic!(concat!("Failed to convert named argument \"", stringify!(#arg_names), "\": {}"), error)
                                                    })
                                                };
                                            )*

                                            Arguments {
                                                #(
                                                    #arg_names,
                                                )*
                                            }
                                        }
                                    }
                                };

                                let _ret = instance.#func_name(#(args.#arg_names,)*);

                                if #is_by_ref && #is_mut {
                                    use casper_contract_sdk::FieldStateAccess;
                                    let _ = instance.write_state_to_fields().unwrap();
                                }

                                #handle_ret
                            }

                            #[cfg(not(target_arch = "wasm32"))]
                            #vis fn #dispatch_func_name<T>()
                            where
                                T: #trait_name
                                    + #crate_path::serializers::borsh::BorshDeserialize
                                    + #crate_path::serializers::borsh::BorshSerialize
                                    + #crate_path::FieldStateAccess
                                    + Default
                            {
                                use casper_contract_sdk::FieldStateAccess;

                                #[derive(#crate_path::serializers::borsh::BorshDeserialize, Debug)]
                                #[borsh(crate = #borsh_path)]
                                struct Arguments {
                                    #(#args_attrs,)*
                                }

                                let mut flags = #crate_path::common::flags::ReturnFlags::empty();
                                let mut instance: T = T::read_state_from_fields().unwrap();
                                let input = #crate_path::prelude::casper::copy_input();
                                let args: Arguments = {
                                    match #resolve_abi_convention {
                                        casper_contract_sdk::serializers::AbiConvention::Positional => {
                                            casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap()
                                        }
                                        casper_contract_sdk::serializers::AbiConvention::Named => {
                                            let runtime_args: casper_contract_sdk::compat::types::RuntimeArgs =
                                                casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap();
                                            #(
                                                let #arg_names: #arg_types = {
                                                    let cl_value = runtime_args.get(stringify!(#arg_names)).unwrap_or_else(|| panic!(concat!("Failed to get named argument \"", stringify!(#arg_names), "\"")));
                                                    cl_value.to_t::<#arg_types>().unwrap_or_else(|error| {
                                                        panic!(concat!("Failed to convert named argument \"", stringify!(#arg_names), "\": {}"), error)
                                                    })
                                                };
                                            )*

                                            Arguments {
                                                #(
                                                    #arg_names,
                                                )*
                                            }
                                        }
                                    }
                                };

                                let _ret = instance.#func_name(#(args.#arg_names,)*);

                                if #is_by_ref && #is_mut {
                                    use casper_contract_sdk::FieldStateAccess;
                                    let _ = instance.write_state_to_fields().unwrap();
                                }

                                #handle_ret
                            }
                        }
                    }

                    None | Some(syn::FnArg::Typed(_)) => {
                        assert!(
                            !method_attribute.private,
                            "can't make dispatcher for private static method"
                        );
                        quote! {
                            #[cfg(not(target_arch = "wasm32"))]
                            #vis fn #dispatch_func_name<T: #trait_name>() {
                                #[derive(#crate_path::serializers::borsh::BorshDeserialize)]
                                #[borsh(crate = #borsh_path)]
                                struct Arguments {
                                    #(#args_attrs,)*
                                }

                                let mut flags = #crate_path::common::flags::ReturnFlags::empty();
                                let input = #crate_path::prelude::casper::copy_input();
                                let args: Arguments = {
                                    match #resolve_abi_convention {
                                        casper_contract_sdk::serializers::AbiConvention::Positional => {
                                            casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap()
                                        }
                                        casper_contract_sdk::serializers::AbiConvention::Named => {
                                            let runtime_args: casper_contract_sdk::compat::types::RuntimeArgs =
                                                casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap();
                                                #(
                                                    let #arg_names: #arg_types = {
                                                        let cl_value = runtime_args.get(stringify!(#arg_names)).unwrap_or_else(|| panic!(concat!("Failed to get named argument \"", stringify!(#arg_names), "\"")));
                                                        cl_value.to_t::<#arg_types>().unwrap_or_else(|error| {
                                                            panic!(concat!("Failed to convert named argument \"", stringify!(#arg_names), "\": {}"), error)
                                                        })
                                                    };
                                                )*

                                            Arguments {
                                                #(
                                                    #arg_names,
                                                )*
                                            }
                                        }
                                    }
                                };
                                let _ret = <T as #trait_name>::#func_name(#(args.#arg_names,)*);

                                #handle_ret
                            }

                            #[cfg(target_arch = "wasm32")]
                            #vis extern "C" fn #dispatch_func_name<T: #trait_name>() {
                                #[derive(#crate_path::serializers::borsh::BorshDeserialize)]
                                #[borsh(crate = #borsh_path)]
                                struct Arguments {
                                    #(#args_attrs,)*
                                }

                                let mut flags = #crate_path::common::flags::ReturnFlags::empty();
                                let input = #crate_path::prelude::casper::copy_input();
                                let args: Arguments = {
                                    match #resolve_abi_convention {
                                        casper_contract_sdk::serializers::AbiConvention::Positional => {
                                            casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap()
                                        }
                                        casper_contract_sdk::serializers::AbiConvention::Named => {
                                            let runtime_args: casper_contract_sdk::compat::types::RuntimeArgs =
                                                casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap();
                                                #(
                                                    let #arg_names: #arg_types = {
                                                        let cl_value = runtime_args.get(stringify!(#arg_names)).unwrap_or_else(|| panic!(concat!("Failed to get named argument \"", stringify!(#arg_names), "\"")));
                                                        cl_value.to_t::<#arg_types>().unwrap_or_else(|error| {
                                                            panic!(concat!("Failed to convert named argument \"", stringify!(#arg_names), "\": {}"), error)
                                                        })
                                                    };
                                                )*

                                            Arguments {
                                                #(
                                                    #arg_names,
                                                )*
                                            }
                                        }
                                    }
                                };
                                let _ret = <T as #trait_name>::#func_name(#(args.#arg_names,)*);

                                #handle_ret
                            }
                        }
                    }
                };

                let is_constructor = method_attribute.constructor;
                let is_payable = method_attribute.payable;

                let abi_receiver = match parse_abi_receiver(&func.sig) {
                    Ok(value) => value,
                    Err(value) => return value,
                };

                macro_symbols.push(quote! {
                    @exportas #export_name, @is_constructor #is_constructor, @is_payable #is_payable, @abi_convention #abi_convention, @receiver #abi_receiver, #vis fn #dispatch_func_name ( #(#arg_names: #arg_types,)* ) -> #ret_ty;
                });

                dispatch_functions.push(quote! { #handle_dispatch });

                let input_data_content = if arg_names.is_empty() {
                    quote! {
                        None
                    }
                } else {
                    quote! {
                        Some(#crate_path::serializers::borsh::to_vec(&self).expect("Serialization to succeed"))
                    }
                };
                let self_ty = if method_attribute.constructor || method_attribute.ignore_state {
                    None
                } else {
                    Some(quote! {
                        self,
                    })
                };

                extra_code.push(quote! {
                    fn #func_name<'a>(#self_ty #(#arg_names: #arg_types,)*) -> impl #crate_path::ToCallData<Return<'a> = #call_data_return_lifetime> {
                        #[derive(#crate_path::serializers::borsh::BorshSerialize)]
                        #[borsh(crate = #borsh_path)]
                        struct CallData {
                            #(pub #arg_names: #arg_types,)*
                        }

                        impl #crate_path::ToCallData for CallData {
                            type Return<'a> = #call_data_return_lifetime;

                            fn entry_point(&self) -> &str { stringify!(#export_name) }
                            fn input_data(&self) -> Option<#crate_path::prelude::vec::Vec<u8>> {
                                #input_data_content
                            }
                        }

                        CallData {
                            #(#arg_names,)*
                        }
                    }
                    });
            }
            syn::TraitItem::Type(_) => {
                return syn::Error::new(Span::call_site(), "Unsupported generic associated types")
                    .to_compile_error()
                    .into();
            }
            syn::TraitItem::Macro(_) => todo!("Macro"),
            syn::TraitItem::Verbatim(_) => todo!("Verbatim"),
            other => todo!("Other {other:?}"),
        }
    }
    let ref_struct = format_ident!("{trait_name}Ref");
    let ext_struct_trait = format_ident!("{trait_name}Ext");

    let macro_name = format_ident!("enumerate_{trait_name}_symbols");

    let abi_conv = match trait_meta.abi_convention.as_ref() {
        Some(ref convention) => {
            quote! {
                impl #crate_path::serializers::AbiConfig for #ref_struct {
                    const DEFAULT_ABI_CONVENTION: #crate_path::serializers::AbiConvention = #convention;
                }
            }
        }
        None => quote! {
            impl #crate_path::serializers::AbiConfig for #ref_struct {
               const DEFAULT_ABI_CONVENTION: #crate_path::serializers::AbiConvention = #crate_path::serializers::AbiConvention::Positional;
            }
        },
    };

    let maybe_exported_macro = if !trait_meta.export.unwrap_or(false) {
        quote! {
            #[allow(non_snake_case, unused_macros)]
            macro_rules! #macro_name {
                ($mac:ident) => {
                    $mac! {
                        #(#macro_symbols)*
                    }
                }
            }
            pub(crate) use #macro_name;
        }
    } else {
        quote! {
            #[allow(non_snake_case, unused_macros)]
            #[macro_export]
            macro_rules! #macro_name {
                ($mac:ident) => {
                    $mac! {
                        #(#macro_symbols)*
                    }
                }
            }
        }
    };

    let extension_struct = quote! {
        #vis trait #ext_struct_trait: Sized {
            #(#extra_code)*
        }

        #vis struct #ref_struct;

        impl #ref_struct {

        }

        #maybe_exported_macro

        #(#dispatch_functions)*

        #abi_conv

        impl #ext_struct_trait for #ref_struct {}
            impl #crate_path::ContractRef for #ref_struct {
                fn new() -> Self {
                    #ref_struct
                }
            }
    };
    quote! {
        #item_trait

        #extension_struct
    }
    .into()
}

fn parse_abi_receiver(sig: &syn::Signature) -> Result<proc_macro2::TokenStream, TokenStream> {
    let abi_receiver = match sig.inputs.first() {
        Some(syn::FnArg::Receiver(receiver)) if receiver.mutability.is_some() => {
            if receiver.reference.is_some() {
                quote! { casper_contract_sdk::abi::collector::AbiReceiver::ByMutRef }
            } else {
                quote! { casper_contract_sdk::abi::collector::AbiReceiver::ByVal }
            }
        }
        Some(syn::FnArg::Receiver(receiver)) if receiver.mutability.is_none() => {
            quote! { casper_contract_sdk::abi::collector::AbiReceiver::ByRef }
        }

        Some(syn::FnArg::Receiver(receiver)) if receiver.lifetime().is_some() => {
            return Err(TokenStream::from(
                syn::Error::new(
                    Span::call_site(),
                    "Lifetimes are currently not supported in entry points",
                )
                .to_compile_error(),
            ));
        }
        Some(_) | None => {
            quote! { casper_contract_sdk::abi::collector::AbiReceiver::NoReceiver }
        }
    };
    Ok(abi_receiver)
}

fn generate_casper_state_for_struct(
    item_struct: &ItemStruct,
    struct_meta: StructMeta,
) -> impl quote::ToTokens {
    let crate_path = match &struct_meta.path {
        Some(path) => quote! { #path },
        None => quote! { casper_contract_sdk },
    };

    let crate_path_str = match &struct_meta.path {
        Some(path) => path.to_token_stream().to_string(),
        None => "casper_contract_sdk".to_string(),
    };

    let borsh_path = {
        syn::LitStr::new(
            &format!("{}::serializers::borsh", crate_path_str),
            Span::call_site(),
        )
    };
    let maybe_derive_abi = get_maybe_derive_abi(crate_path.clone());

    let struct_name = &item_struct.ident;

    quote! {
        #[derive(#crate_path::serializers::borsh::BorshSerialize, #crate_path::serializers::borsh::BorshDeserialize, #crate_path::macros::TypeUid)]
        #[type_uid(crate = #crate_path::common::type_uid)]
        #[borsh(crate = #borsh_path)]
        #maybe_derive_abi
        #item_struct

        impl #crate_path::compat::types::CLTyped for #struct_name {
            fn cl_type() -> #crate_path::compat::types::CLType {
                #crate_path::compat::types::CLType::Any
            }
        }
    }
}

fn generate_casper_state_for_enum(
    item_enum: &ItemEnum,
    enum_meta: EnumMeta,
) -> impl quote::ToTokens {
    let crate_path = match &enum_meta.path {
        Some(path) => quote! { #path },
        None => quote! { casper_contract_sdk },
    };

    let borsh_path = {
        let crate_path_str = match &enum_meta.path {
            Some(path) => path.to_token_stream().to_string(),
            None => "casper_contract_sdk".to_string(),
        };
        syn::LitStr::new(
            &format!("{}::serializers::borsh", crate_path_str),
            Span::call_site(),
        )
    };

    let maybe_derive_abi = get_maybe_derive_abi(crate_path.clone());

    let enum_name = &item_enum.ident;

    quote! {
        #[derive(#crate_path::serializers::borsh::BorshSerialize, #crate_path::serializers::borsh::BorshDeserialize, #crate_path::macros::TypeUid)]
        #[type_uid(crate = #crate_path::common::type_uid)]
        #[borsh(use_discriminant = true, crate = #borsh_path)]
        #[repr(u32)]
        #maybe_derive_abi
        #item_enum

        impl #crate_path::compat::types::CLTyped for #enum_name {
            fn cl_type() -> #crate_path::compat::types::CLType {
                #crate_path::compat::types::CLType::Any
            }
        }
    }
}

fn get_maybe_derive_abi(_crate_path: impl ToTokens) -> impl ToTokens {
    {
        quote! {
            #[cfg_attr(not(target_arch = "wasm32"), derive(#_crate_path::macros::CasperABI))]
        }
    }
}

fn process_casper_contract_state_for_struct(
    contract_struct: &ItemStruct,
    struct_meta: StructMeta,
) -> TokenStream {
    let struct_name = &contract_struct.ident;
    let ref_name = format_ident!("{struct_name}Ref");
    let vis = &contract_struct.vis;

    let crate_path = match &struct_meta.path {
        Some(path) => quote! { #path },
        None => quote! { casper_contract_sdk },
    };
    let borsh_path = {
        let crate_path_str = match &struct_meta.path {
            Some(path) => path.to_token_stream().to_string(),
            None => "casper_contract_sdk".to_string(),
        };
        syn::LitStr::new(
            &format!("{}::serializers::borsh", crate_path_str),
            Span::call_site(),
        )
    };

    let maybe_derive_abi = get_maybe_derive_abi(crate_path.clone());

    // let convention = struct_meta.abi_convention.unwrap_or(quote! {
    // #crate_path::serializers::AbiConvention::Positional });
    let abi_conv = match struct_meta.abi_convention {
        Some(convention) => {
            quote! {
                #convention
            }
        }
        None => quote! {
            #crate_path::serializers::AbiConvention::Positional
        },
    };

    // Build per-field read/write code for named fields
    let (read_bindings, write_statements, init_fields) = match &contract_struct.fields {
        syn::Fields::Named(fields) => {
            let mut reads = Vec::new();
            let mut writes = Vec::new();
            let mut inits = Vec::new();
            for field in &fields.named {
                if let Some(field_ident) = &field.ident {
                    let field_ty = &field.ty;
                    reads.push(quote! {
                        let #field_ident: #field_ty = {
                            const FIELD_NAME: &'static str = stringify!(#field_ident);
                            let state_addr = #crate_path::common::keyspace::StateAddrInner::new(
                                FIELD_NAME,
                            );
                            let mut buf = #crate_path::prelude::Vec::new();
                            let info = #crate_path::casper::read(
                                #crate_path::common::keyspace::Keyspace::Context(
                                    #crate_path::common::keyspace::ContextAddr::from(state_addr)
                                ),
                                |sz| #crate_path::reserve_vec_space(&mut buf, sz)
                            )?;
                            if let Some(()) = info {
                                #crate_path::serializers::borsh::from_slice(&buf).unwrap()
                            } else {
                                return Err(#crate_path::common::error::HostResult::NotFound);
                            }
                        };
                    });
                    writes.push(quote! {
                        {
                            const FIELD_NAME: &'static str = stringify!(#field_ident);
                            let state_addr = #crate_path::common::keyspace::StateAddrInner::new(
                                FIELD_NAME,
                            );
                            let bytes = #crate_path::serializers::borsh::to_vec(&self.#field_ident).unwrap();
                            #crate_path::casper::write(
                                #crate_path::common::keyspace::Keyspace::Context(
                                    #crate_path::common::keyspace::ContextAddr::from(state_addr)
                                ),
                                &bytes
                            )?;
                        }
                    });
                    inits.push(quote! { #field_ident, });
                }
            }
            (reads, writes, inits)
        }
        _ => (Vec::new(), Vec::new(), Vec::new()),
    };

    quote! {
        #[derive(#crate_path::serializers::borsh::BorshSerialize, #crate_path::serializers::borsh::BorshDeserialize, #crate_path::macros::TypeUid)]
        #[borsh(crate = #borsh_path)]
        #maybe_derive_abi
        #contract_struct

        #vis struct #ref_name;

        impl #crate_path::serializers::AbiConfig for #struct_name {
            const DEFAULT_ABI_CONVENTION: #crate_path::serializers::AbiConvention = #abi_conv;
        }

        impl #crate_path::ContractRef for #ref_name {
            fn new() -> Self {
                #ref_name
            }
        }

        impl #crate_path::compat::types::CLTyped for #struct_name {
            fn cl_type() -> #crate_path::compat::types::CLType {
                #crate_path::compat::types::CLType::Any
            }
        }

        impl #crate_path::FieldStateAccess for #struct_name {
            fn read_state_from_fields() -> Result<Self, #crate_path::common::error::HostResult> {
                #(#read_bindings)*
                Ok(Self { #(#init_fields)* })
            }

            fn write_state_to_fields(&self) -> Result<(), #crate_path::common::error::HostResult> {
                #(#write_statements)*
                Ok(())
            }
        }

        #[cfg(not(target_arch = "wasm32"))]
        const _: () = {
            #[casper_contract_sdk::linkme::distributed_slice(casper_contract_sdk::abi::collector::ABI_ITEMS)]
            #[linkme(crate = casper_contract_sdk::linkme)]
            pub static ABI_ITEM: casper_contract_sdk::abi::collector::AbiItem = casper_contract_sdk::abi::collector::AbiItem::SmartContract(casper_contract_sdk::abi::collector::AbiSmartContract {
                struct_name: stringify!(#struct_name),
                abi_convention: #abi_conv,
                decl: #crate_path::abi::collector::AbiType {
                    type_name: core::any::type_name::<#struct_name>,
                    type_id: #crate_path::common::type_uid::of::<#struct_name>(),
                    cl_type: <#struct_name as #crate_path::compat::types::CLTyped>::cl_type,
                    visit_abi_types: |visitor| {
                        #crate_path::abi::visit_types_recursively::<#struct_name>(visitor);
                    },
                },
                metadata: || {
                    #crate_path::prelude::collections::BTreeMap::from_iter([
                        ("CARGO", option_env!("CARGO")),
                        ("CARGO_MANIFEST_DIR", option_env!("CARGO_MANIFEST_DIR")),
                        ("CARGO_MANIFEST_PATH", option_env!("CARGO_MANIFEST_PATH")),
                        ("CARGO_PKG_VERSION", option_env!("CARGO_PKG_VERSION")),
                        ("CARGO_PKG_VERSION_MAJOR", option_env!("CARGO_PKG_VERSION_MAJOR")),
                        ("CARGO_PKG_VERSION_MINOR", option_env!("CARGO_PKG_VERSION_MINOR")),
                        ("CARGO_PKG_VERSION_PATCH", option_env!("CARGO_PKG_VERSION_PATCH")),
                        ("CARGO_PKG_VERSION_PRE", option_env!("CARGO_PKG_VERSION_PRE")),
                        ("CARGO_PKG_AUTHORS", option_env!("CARGO_PKG_AUTHORS")),
                        ("CARGO_PKG_NAME", option_env!("CARGO_PKG_NAME")),
                        ("CARGO_PKG_DESCRIPTION", option_env!("CARGO_PKG_DESCRIPTION")),
                        ("CARGO_PKG_HOMEPAGE", option_env!("CARGO_PKG_HOMEPAGE")),
                        ("CARGO_PKG_REPOSITORY", option_env!("CARGO_PKG_REPOSITORY")),
                        ("CARGO_PKG_LICENSE", option_env!("CARGO_PKG_LICENSE")),
                        ("CARGO_PKG_LICENSE_FILE", option_env!("CARGO_PKG_LICENSE_FILE")),
                        ("CARGO_PKG_RUST_VERSION", option_env!("CARGO_PKG_RUST_VERSION")),
                        ("CARGO_PKG_README", option_env!("CARGO_PKG_README")),
                        ("CARGO_CRATE_NAME", option_env!("CARGO_CRATE_NAME")),
                        ("CARGO_BIN_NAME", option_env!("CARGO_BIN_NAME")),
                        ("OUT_DIR", option_env!("OUT_DIR")),
                        ("CARGO_PRIMARY_PACKAGE", option_env!("CARGO_PRIMARY_PACKAGE")),
                        ("CARGO_TARGET_TMPDIR", option_env!("CARGO_TARGET_TMPDIR"))])
                    }
                }
            );
        };
    }
    .into()
}

#[proc_macro_attribute]
pub fn entry_point(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let func = parse_macro_input!(item as ItemFn);

    let vis = &func.vis;
    let _sig = &func.sig;
    let func_name = &func.sig.ident;

    let block = &func.block;

    let mut handle_args = Vec::new();
    let mut params = Vec::new();

    for arg in &func.sig.inputs {
        let typed = match arg {
            syn::FnArg::Receiver(_) => todo!(),
            syn::FnArg::Typed(typed) => typed,
        };

        let name = match typed.pat.as_ref() {
            syn::Pat::Ident(ident) => &ident.ident,
            _ => todo!(),
        };

        let ty = &typed.ty;

        let tok = quote! {
            let #typed = casper_contract_sdk::get_named_arg(stringify!(#name)).expect("should get named arg");
        };
        handle_args.push(tok);

        let tok2 = quote! {
            (stringify!(#name), <#ty>::cl_type())
        };
        params.push(tok2);
    }

    // let len = params.len();

    let output = &func.sig.output;

    // let const_tok =

    let gen = quote! {
        // const paste!(#func_name, _ENTRY_POINT): &str = #func_name;

        #vis fn #func_name() {
            #(#handle_args)*;

            let closure = || #output {
                #block
            };

            let result = closure();

            // casper_contract_sdk::EntryPoint {
            //     name: #func_name,
            //     params: &[
            //         #(#params,)*
            //     ],
            //     func: closure,
            // }

            result.expect("should work")
        }
    };

    println!("{gen}");

    // quote!(fn foo() {})
    // item
    gen.into()
}

// #[proc_macro_derive(CasperSchema, attributes(casper))]
// pub fn derive_casper_schema(input: TokenStream) -> TokenStream {
//     let contract = parse_macro_input!(input as DeriveInput);

//     let contract_attributes = ContractAttributes::from_attributes(&contract.attrs).unwrap();

//     let _data_struct = match &contract.data {
//         Data::Struct(s) => s,
//         Data::Enum(_) => todo!("Enum"),
//         Data::Union(_) => todo!("Union"),
//     };

//     let name = &contract.ident;

//     // let mut extra_code = Vec::new();
//     // if let Some(traits) = contract_attributes.impl_traits {
//     //     for path in traits.iter() {
//     //         let ext_struct = format_ident!("{}Ref", path.require_ident().unwrap());
//     //         extra_code.push(quote! {
//     //             {
//     //                 let entry_points = <#ext_struct>::__casper_schema_entry_points();
//     //                 schema.entry_points.extend(entry_points);
//     //                 <#ext_struct>::__casper_populate_definitions(&mut schema.definitions);
//     //             }
//     //         });
//     //     }

//     //     let macro_name = format_ident!("enumerate_{path}_symbols");

//     //     extra_code.push(quote! {
//     //         const _: () = {
//     //             macro_rules! #macro_name {
//     //                 ($mac:ident) => {
//     //                     $mac! {
//     //                         #(#extra_code)*
//     //                     }
//     //                 }
//     //             }
//     //         }
//     //     })
//     // }

//     quote! {
//         impl casper_contract_sdk::schema::CasperSchema for #name {
//             fn schema() -> casper_contract_sdk::schema::Schema {
//                 let mut schema = Self::__casper_schema();

//                 // #(#extra_code)*;

//                 schema
//                 // schema.entry_points.ext
//             }
//         }
//     }
//     .into()
// }

#[proc_macro_derive(CasperABI, attributes(casper))]
pub fn derive_casper_abi(input: TokenStream) -> TokenStream {
    let res = if let Ok(input) = syn::parse::<ItemStruct>(input.clone()) {
        let mut populate_visitor = Vec::new();
        let name = input.ident.clone();
        let mut items = Vec::new();
        for field in &input.fields {
            match &field.ty {
                Type::Path(path) => {
                    for segment in &path.path.segments {
                        let field_name = &field.ident;

                        populate_visitor.push(quote! {
                            <#segment>::visit(visitor);
                        });

                        items.push(quote! {
                            casper_contract_sdk::abi::StructField {
                                name: stringify!(#field_name).into(),
                                decl: casper_contract_sdk::common::type_uid::of::<#segment>().into(),
                            }
                        });
                    }
                }
                other_ty => todo!("Unsupported type {other_ty:?}"),
            }
        }

        Ok(quote! {
            impl casper_contract_sdk::abi::CasperABI for #name {
                fn visit(visitor: &mut dyn casper_contract_sdk::abi::ABIVisitor) {
                    visitor.accept(casper_contract_sdk::abi::ABITypeInfo::from_abi_type::<#name>());
                    #(#populate_visitor)*;
                }

                fn declaration() -> casper_contract_sdk::abi::AbiDeclaration {
                    core::any::type_name::<#name>().into()
                }

                fn definition() -> casper_contract_sdk::abi::Definition {
                    casper_contract_sdk::abi::Definition::Struct {
                        items: vec![
                            #(#items,)*
                        ]
                    }
                }
            }
        })
    } else if let Ok(input) = syn::parse::<ItemEnum>(input.clone()) {
        // TODO: Check visibility
        let name = input.ident.clone();

        let mut all_variants = Vec::new();
        let mut populate_definitions = Vec::new();
        let mut has_unit_definition = false;

        let mut current_discriminant = 0;

        for variant in &input.variants {
            if let Some(discriminant) = &variant.discriminant {
                match &discriminant.1 {
                    syn::Expr::Lit(lit) => match &lit.lit {
                        syn::Lit::Int(int) => {
                            current_discriminant = int.base10_parse::<u64>().unwrap();
                        }
                        _ => todo!(),
                    },
                    _ => todo!(),
                }
            }

            let variant_name = &variant.ident;

            let variant_decl = match &variant.fields {
                Fields::Unit => {
                    // NOTE: Generate an empty struct here for a definition.
                    if !has_unit_definition {
                        has_unit_definition = true;
                    }

                    quote! {
                        Some(casper_contract_sdk::common::type_uid::of::<()>().into())
                    }
                }
                Fields::Named(named) => {
                    let mut fields = Vec::new();

                    for field in &named.named {
                        let field_name = &field.ident;
                        match &field.ty {
                            Type::Path(path) => {
                                fields.push(quote! {
                                    casper_contract_sdk::abi::StructField {
                                        name: stringify!(#field_name).into(),
                                        decl: casper_contract_sdk::common::type_uid::of::<#path>().into(),
                                    }
                                });
                            }
                            other_ty => todo!("Unsupported type {other_ty:?}"),
                        }
                    }

                    quote! {
                        // Plain enum variants don't require a type declaration.
                        None
                    }
                }
                Fields::Unnamed(unnamed_fields) => {
                    let mut fields = Vec::new();

                    let _variant_name = format_ident!("{name}_{variant_name}");

                    for field in &unnamed_fields.unnamed {
                        match &field.ty {
                            Type::Path(path) => {
                                for segment in &path.path.segments {
                                    let type_name = &segment.ident;
                                    populate_definitions.push(quote! {
                                        <#type_name>::visit(visitor);
                                    });

                                    fields.push(quote! {
                                        casper_contract_sdk::common::type_uid::of::<#type_name>()
                                    });
                                }
                            }
                            other_ty => todo!("Unsupported type {other_ty:?}"),
                        }
                    }

                    quote! {
                        // TODO: Deal with newtypes
                        None
                    }
                }
            };

            all_variants.push(quote! {
                casper_contract_sdk::abi::EnumVariant {
                    name: stringify!(#variant_name).into(),
                    discriminant: #current_discriminant,
                    decl: #variant_decl,
                }
            });

            current_discriminant += 1;
        }

        Ok(quote! {
            impl casper_contract_sdk::abi::CasperABI for #name {
                fn visit(visitor: &mut dyn casper_contract_sdk::abi::ABIVisitor) {
                    visitor.accept(casper_contract_sdk::abi::ABITypeInfo::from_abi_type::<#name>());
                    #(#populate_definitions)*;
                }

                fn declaration() -> casper_contract_sdk::abi::AbiDeclaration {
                    core::any::type_name::<#name>().into()
                }

                fn definition() -> casper_contract_sdk::abi::Definition {
                    casper_contract_sdk::abi::Definition::Enum {
                        items: vec![
                            #(#all_variants,)*
                        ],
                    }
                }
            }
        })
    } else if syn::parse::<ItemUnion>(input).is_ok() {
        Err(syn::Error::new(
            Span::call_site(),
            "Borsh schema does not support unions yet.",
        ))
    } else {
        // Derive macros can only be defined on structs, enums, and unions.
        unreachable!()
    };
    TokenStream::from(match res {
        Ok(res) => res,
        Err(err) => err.to_compile_error(),
    })
}

#[proc_macro]
pub fn blake2b256(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);
    let bytes = input.value();

    let hash = utils::compute_blake2b256(bytes.as_bytes());

    TokenStream::from(quote! {
        [ #(#hash),* ]
    })
}

#[proc_macro]
pub fn test(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    TokenStream::from(quote! {
        #[test]
        #input
    })
}

/// `PanicOnDefault` generates implementation for `Default` trait that panics with the following
/// message `The contract is not initialized` when `default()` is called.
///
/// This is to protect againsts default-initialization of contracts in a situation where no
/// constructor is called, and an entrypoint is invoked before the contract is initialized.
#[proc_macro_derive(PanicOnDefault)]
pub fn derive_no_default(item: TokenStream) -> TokenStream {
    if let Ok(input) = syn::parse::<ItemStruct>(item) {
        let name = &input.ident;
        TokenStream::from(quote! {
            impl ::core::default::Default for #name {
                fn default() -> Self {
                    panic!("The contract is not initialized");
                }
            }
        })
    } else {
        TokenStream::from(
            syn::Error::new(
                Span::call_site(),
                "PanicOnDefault can only be used on type declarations sections.",
            )
            .to_compile_error(),
        )
    }
}

#[proc_macro_derive(TypeUid, attributes(type_uid))]
pub fn derive_type_uid(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    // Extract crate path from #[type_uid(crate = "crate_path")] attribute
    let crate_path = input
        .attrs
        .iter()
        .find_map(|attr| {
            if attr.path().is_ident("type_uid") {
                attr.parse_args::<syn::Meta>().ok().and_then(|meta| {
                    if let syn::Meta::NameValue(nv) = meta {
                        if nv.path.is_ident("crate") {
                            if let syn::Expr::Lit(expr_lit) = nv.value {
                                if let syn::Lit::Str(lit_str) = expr_lit.lit {
                                    return Some(lit_str.value());
                                }
                            }
                        }
                    }
                    None
                })
            } else {
                None
            }
        })
        .unwrap_or_else(|| "casper_contract_sdk::common::type_uid".to_string());

    let crate_path_token: proc_macro2::TokenStream = crate_path.parse().unwrap();

    match &input.data {
        syn::Data::Struct(ds) => {
            let fields = &ds.fields;

            let mut mixer = Vec::new();

            for field in fields.iter() {
                let ty = &field.ty;

                mixer.push(quote! {
                    <#ty>::UID
                });
            }

            TokenStream::from(quote! {
                impl #crate_path_token::TypeUid for #name {
                    const UID: #crate_path_token::Uid = #crate_path_token::Uid::from_fields(
                        stringify!(#name),
                        &[
                            #(#mixer,)*
                        ]
                    );
                }
            })
        }
        syn::Data::Enum(de) => {
            let mut mixer = Vec::new();

            for variant in de.variants.iter() {
                let variant_name = &variant.ident;

                let mut field_mixers = Vec::new();

                for field in variant.fields.iter() {
                    let ty = &field.ty;

                    field_mixers.push(quote! {
                        <#ty>::UID
                    });
                }
                if let Some((_, discriminant)) = &variant.discriminant {
                    field_mixers.push(quote! { #crate_path_token::Uid::new_raw(#discriminant) });
                }

                mixer.push(quote! {
                    #crate_path_token::Uid::from_fields(
                        stringify!(#variant_name),
                        &[
                            #(#field_mixers,)*
                        ]
                    )
                });
            }

            TokenStream::from(quote! {
                impl #crate_path_token::TypeUid for #name {
                    const UID: #crate_path_token::Uid = #crate_path_token::Uid::from_fields(
                        stringify!(#name),
                        &[#(#mixer,)*]
                    );
                }
            })
        }
        syn::Data::Union(_) => TokenStream::from(
            syn::Error::new(Span::call_site(), "TypeUid cannot be derived for unions")
                .to_compile_error(),
        ),
    }
}
