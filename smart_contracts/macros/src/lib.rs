pub(crate) mod utils;

extern crate proc_macro;

use darling::{ast, FromAttributes, FromMeta};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote, ToTokens};
use syn::{
    parse_macro_input, Fields, ItemEnum, ItemFn, ItemImpl, ItemStruct, ItemTrait, ItemUnion,
    LitStr, Type,
};

use casper_executor_wasm_common::flags::EntryPointFlags;
const CASPER_RESERVED_FALLBACK_EXPORT: &str = "__casper_fallback";

#[derive(Debug, FromAttributes)]
#[darling(attributes(casper))]
struct MethodAttribute {
    #[darling(default)]
    constructor: bool,
    #[darling(default)]
    ignore_state: bool,
    #[darling(default)]
    revert_on_error: bool,
    /// Explicitly mark method as private so it's not externally callable.
    #[darling(default)]
    private: bool,
    #[darling(default)]
    payable: bool,
    #[darling(default)]
    fallback: bool,
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
    message: bool,
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

    let has_fallback_selector = false;

    if let Ok(item_struct) = syn::parse::<ItemStruct>(item.clone()) {
        let struct_meta = StructMeta::from_list(&attr_args).unwrap();
        if struct_meta.message {
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
            generate_impl_for_contract(entry_points, has_fallback_selector)
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

    let maybe_abi_collectors;
    let maybe_entrypoint_defs;

    #[cfg(feature = "__abi_generator")]
    {
        maybe_abi_collectors = quote! {
            const _: () = {
                #[#crate_path::linkme::distributed_slice(#crate_path::abi_generator::ABI_COLLECTORS)]
                #[linkme(crate = #crate_path::linkme)]
                static COLLECTOR: fn(&mut #crate_path::abi::Definitions) = |defs| {
                    defs.populate_one::<#struct_name>();
                };
            };
        };

        maybe_entrypoint_defs = quote! {
            const _: () = {
                #[#crate_path::linkme::distributed_slice(#crate_path::abi_generator::MESSAGES)]
                #[linkme(crate = #crate_path::linkme)]
                static MESSAGE: #crate_path::abi_generator::Message = #crate_path::abi_generator::Message {
                    name: <#struct_name as #crate_path::Message>::TOPIC,
                    decl: concat!(module_path!(), "::", stringify!(#struct_name)),
                 };
            };
        }
    }
    #[cfg(not(feature = "__abi_generator"))]
    {
        maybe_abi_collectors = quote! {};
        maybe_entrypoint_defs = quote! {};
    }

    quote! {
        #[derive(#crate_path::serializers::borsh::BorshSerialize)]
        #[borsh(crate = #borsh_path)]
        #maybe_derive_abi
        #item_struct

        impl #crate_path::Message for #struct_name {
            const TOPIC: &'static str = stringify!(#struct_name);

            #[inline]
            fn payload(&self) -> Vec<u8> {
                #crate_path::serializers::borsh::to_vec(self).unwrap()
            }
        }

        #maybe_abi_collectors
        #maybe_entrypoint_defs

    }
    .into()
}

fn generate_export_function(func: &ItemFn) -> TokenStream {
    let func_name = &func.sig.ident;
    let mut arg_names = Vec::new();
    let mut args_attrs = Vec::new();
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
        args_attrs.push(quote! {
            #name: #ty
        });
    }
    let _ctor_name = format_ident!("{func_name}_ctor");

    let exported_func_name = format_ident!("__casper_export_{func_name}");
    quote! {
        #[export_name = stringify!(#func_name)]
        #[no_mangle]
        pub extern "C" fn #exported_func_name() {
            #[cfg(target_arch = "wasm32")]
            {
                casper_contract_sdk::set_panic_hook();
            }

            #func

            #[derive(casper_contract_sdk::serializers::borsh::BorshDeserialize)]
            #[borsh(crate = "casper_contract_sdk::serializers::borsh")]
            struct Arguments {
                #(#args_attrs,)*
            }
            let input = casper_contract_sdk::prelude::casper::copy_input();
            let args: Arguments = casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap();
            let _ret = #func_name(#(args.#arg_names,)*);
        }

        #[cfg(not(target_arch = "wasm32"))]
        #func

        #[cfg(not(target_arch = "wasm32"))]
        const _: () = {
            #[casper_contract_sdk::linkme::distributed_slice(casper_contract_sdk::casper::native::ENTRY_POINTS)]
            #[linkme(crate = casper_contract_sdk::linkme)]
            pub static EXPORTS: casper_contract_sdk::casper::native::EntryPoint = casper_contract_sdk::casper::native::EntryPoint {
                kind: casper_contract_sdk::casper::native::EntryPointKind::Function { name: stringify!(#func_name) },
                fptr: || { #exported_func_name(); },
                module_path: module_path!(),
                file: file!(),
                line: line!(),
            };
        };
    }.into()
}

fn generate_impl_for_contract(
    mut entry_points: ItemImpl,
    _has_fallback_selector: bool,
) -> TokenStream {
    #[cfg(feature = "__abi_generator")]
    let mut populate_definitions_linkme = Vec::new();
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
    #[cfg(feature = "__abi_generator")]
    let mut defs = defs;
    #[cfg(feature = "__abi_generator")]
    let mut defs_linkme = Vec::new();
    let mut names = Vec::new();
    let mut extern_entry_points = Vec::new();
    let _abi_generator_entry_points = [quote! {}]; // TODO: Dummy element which may not be necessary but is used for expansion later
    let mut manifest_entry_point_enum_variants = Vec::new();
    let mut manifest_entry_point_enum_match_name = Vec::new();
    let mut manifest_entry_point_input_data = Vec::new();
    let mut extra_code = Vec::new();

    for entry_point in &mut entry_points.items {
        let mut populate_definitions = Vec::new();

        let method_attribute;
        let mut flag_value = EntryPointFlags::empty();

        // let selector_value;

        let func = match entry_point {
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

                method_attribute = MethodAttribute::from_attributes(&func.attrs).unwrap();

                func.attrs.clear();

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

                let export_name = if method_attribute.fallback {
                    format_ident!("{}", CASPER_RESERVED_FALLBACK_EXPORT)
                } else {
                    format_ident!("{}", &func_name)
                };

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

                // Entry point has &self or &mut self
                let mut entry_point_requires_state: bool = false;

                let handle_write_state = match func.sig.inputs.first() {
                    Some(syn::FnArg::Receiver(receiver)) if receiver.mutability.is_some() => {
                        entry_point_requires_state = true;

                        if !never_returns && receiver.reference.is_some() {
                            // &mut self does write updated state
                            Some(quote! {
                                casper_contract_sdk::casper::write_state(&instance).unwrap();
                            })
                        } else {
                            // mut self does not write updated state as the
                            // method call
                            // will consume self and there's nothing to persist.
                            None
                        }
                    }
                    Some(syn::FnArg::Receiver(receiver)) if receiver.mutability.is_none() => {
                        entry_point_requires_state = true;

                        // &self does not write state
                        None
                    }
                    Some(syn::FnArg::Receiver(receiver)) if receiver.lifetime().is_some() => {
                        panic!("Lifetimes are currently not supported");
                    }
                    Some(_) | None => {
                        if !never_returns && method_attribute.constructor {
                            Some(quote! {
                                casper_contract_sdk::casper::write_state(&_ret).unwrap();
                            })
                        } else {
                            None
                        }
                    }
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

                let handle_ret = if never_returns {
                    None
                } else {
                    match func.sig.output {
                        syn::ReturnType::Default => {
                            // Do not call casper_return if there is no return value
                            None
                        }
                        _ if method_attribute.constructor => {
                            // Constructor does not return serialized state but is expected to save
                            // state, or explicitly revert.
                            // TODO: Add support for Result<Self, Error> and revert_on_error if
                            // possible.
                            Some(quote! {
                                let _ = flags; // hide the warning
                            })
                        }
                        syn::ReturnType::Type(..) => {
                            // There is a return value so call casper_return.
                            Some(quote! {
                                let ret_bytes = casper_contract_sdk::serializers::borsh::to_vec(&_ret).unwrap();
                                casper_contract_sdk::casper::ret(flags, Some(&ret_bytes));
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
                    let args: Arguments = casper_contract_sdk::serializers::borsh::from_slice(&input).unwrap();
                });

                if method_attribute.constructor {
                    prelude.push(quote! {
                        if casper_contract_sdk::casper::has_state().unwrap() {
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

                let handle_err = if !never_returns && method_attribute.revert_on_error {
                    if let syn::ReturnType::Default = func.sig.output {
                        panic!("Cannot revert on error if there is no return value");
                    }

                    quote! {
                        let _ret: &Result<_, _> = &_ret;
                        if _ret.is_err() {
                            flags |= casper_contract_sdk::casper_executor_wasm_common::flags::ReturnFlags::REVERT;
                        }

                    }
                } else {
                    quote! {}
                };

                let handle_call = if entry_point_requires_state {
                    quote! {
                        let mut instance: #struct_name = casper_contract_sdk::casper::read_state().unwrap();
                        let _ret = instance.#func_name(#(args.#arg_names,)*);
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
                if method_attribute.constructor {
                    flag_value |= EntryPointFlags::CONSTRUCTOR;
                }

                if method_attribute.fallback {
                    flag_value |= EntryPointFlags::FALLBACK;
                }

                let _bits = flag_value.bits();

                let extern_func_name = format_ident!("__casper_export_{func_name}");

                extern_entry_points.push(quote! {

                    #[export_name = stringify!(#export_name)]
                    #vis extern "C" fn #extern_func_name() {
                        // Set panic hook (assumes std is enabled etc.)
                        #[cfg(target_arch = "wasm32")]
                        {
                            casper_contract_sdk::set_panic_hook();
                        }

                        #(#prelude;)*

                        let mut flags = casper_contract_sdk::casper_executor_wasm_common::flags::ReturnFlags::empty();

                        #handle_call;

                        #handle_err;

                        #handle_write_state;

                        #handle_ret;
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    const _: () = {
                        #[casper_contract_sdk::linkme::distributed_slice(casper_contract_sdk::casper::native::ENTRY_POINTS)]
                        #[linkme(crate = casper_contract_sdk::linkme)]
                        pub static EXPORTS: casper_contract_sdk::casper::native::EntryPoint = casper_contract_sdk::casper::native::EntryPoint {
                            kind: casper_contract_sdk::casper::native::EntryPointKind::SmartContract { name: stringify!(#export_name), struct_name: stringify!(#struct_name) },
                            fptr: || -> () { #extern_func_name(); },
                            module_path: module_path!(),
                            file: file!(),
                            line: line!(),
                        };
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

                        if !method_attribute.fallback {
                            extra_code.push(quote! {
                                        pub fn #func_name<'a>(#self_ty #(#arg_names: #arg_types,)*) -> impl casper_contract_sdk::ToCallData<Return<'a> = #call_data_return_lifetime> {
                                            #[derive(casper_contract_sdk::serializers::borsh::BorshSerialize, PartialEq, Debug)]
                                            #[borsh(crate = "casper_contract_sdk::serializers::borsh")]
                                            struct #ident {
                                                #(#arg_names: #arg_types,)*
                                            }

                                            impl casper_contract_sdk::ToCallData for #ident {
                                                // const SELECTOR: vm_common::selector::Selector = vm_common::selector::Selector::new(#selector_value);

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
                    }

                    _ => todo!("Different self_ty currently unsupported"),
                }

                func.clone()
            }
            syn::ImplItem::Type(_) => todo!(),
            syn::ImplItem::Macro(_) => todo!(),
            syn::ImplItem::Verbatim(_) => todo!(),
            _ => todo!(),
        };

        let mut args = Vec::new();

        for input in &func.sig.inputs {
            let typed = match input {
                syn::FnArg::Receiver(_receiver) => continue,
                syn::FnArg::Typed(typed) => typed,
            };
            // typed
            let name = match &typed.pat.as_ref() {
                syn::Pat::Const(_) => todo!("Const"),
                syn::Pat::Ident(ident) => ident,
                syn::Pat::Lit(_) => todo!("Lit"),
                syn::Pat::Macro(_) => todo!("Macro"),
                syn::Pat::Or(_) => todo!("Or"),
                syn::Pat::Paren(_) => todo!("Paren"),
                syn::Pat::Path(_) => todo!("Path"),
                syn::Pat::Range(_) => todo!("Range"),
                syn::Pat::Reference(_) => todo!("Reference"),
                syn::Pat::Rest(_) => todo!("Rest"),
                syn::Pat::Slice(_) => todo!("Slice"),
                syn::Pat::Struct(_) => todo!("Struct"),
                syn::Pat::Tuple(_) => todo!("Tuple"),
                syn::Pat::TupleStruct(_) => todo!("TupleStruct"),
                syn::Pat::Type(_) => todo!("Type"),
                syn::Pat::Verbatim(_) => todo!("Verbatim"),
                syn::Pat::Wild(_) => todo!("Wild"),
                _ => todo!(),
            };
            let ty = &typed.ty;

            populate_definitions.push(quote! {
                definitions.populate_one::<#ty>();
            });

            args.push(quote! {
                casper_contract_sdk::schema::SchemaArgument {
                    name: stringify!(#name).into(),
                    decl: <#ty as casper_contract_sdk::abi::CasperABI>::declaration(),
                }
            });
        }

        #[cfg(feature = "__abi_generator")]
        {
            let bits = flag_value.bits();

            let result = match &func.sig.output {
                syn::ReturnType::Default => {
                    populate_definitions.push(quote! {
                        definitions.populate_one::<()>();
                    });

                    quote! { <() as casper_contract_sdk::abi::CasperABI>::declaration() }
                }
                syn::ReturnType::Type(_, ty) => match ty.as_ref() {
                    Type::Never(_) => {
                        populate_definitions.push(quote! {
                            definitions.populate_one::<()>();
                        });

                        quote! { <() as casper_contract_sdk::abi::CasperABI>::declaration() }
                    }
                    _ => {
                        populate_definitions.push(quote! {
                            definitions.populate_one::<#ty>();
                        });

                        quote! { <#ty as casper_contract_sdk::abi::CasperABI>::declaration() }
                    }
                },
            };

            let func_name = &func.sig.ident;

            let linkme_schema_entry_point_ident =
                format_ident!("__casper_schema_entry_point_{func_name}");

            defs.push(quote! {
                fn #linkme_schema_entry_point_ident() -> casper_contract_sdk::schema::SchemaEntryPoint {
                    casper_contract_sdk::schema::SchemaEntryPoint {
                        name: stringify!(#func_name).into(),
                        arguments: vec![ #(#args,)* ],
                        result: #result,
                        flags: casper_contract_sdk::casper_executor_wasm_common::flags::EntryPointFlags::from_bits(#bits).unwrap(),
                    }
                }
            });
            defs_linkme.push(linkme_schema_entry_point_ident);

            let linkme_abi_populate_defs_ident =
                format_ident!("__casper_populate_definitions_{func_name}");

            defs.push(quote! {
                fn #linkme_abi_populate_defs_ident(definitions: &mut casper_contract_sdk::abi::Definitions) {
                    #(#populate_definitions)*;
                }
            });

            populate_definitions_linkme.push(linkme_abi_populate_defs_ident);
        }
    }
    // let entry_points_len = entry_points.len();
    let st_name = struct_name.get_ident().unwrap();
    let maybe_abi_collectors;
    let maybe_entrypoint_defs;
    #[cfg(feature = "__abi_generator")]
    {
        maybe_abi_collectors = quote! {
            #(
                const _: () = {
                    #[casper_contract_sdk::linkme::distributed_slice(casper_contract_sdk::abi_generator::ABI_COLLECTORS)]
                    #[linkme(crate = casper_contract_sdk::linkme)]
                    static COLLECTOR: fn(&mut casper_contract_sdk::abi::Definitions) = <#struct_name>::#populate_definitions_linkme;
                };
            )*
        };

        maybe_entrypoint_defs = quote! {
            #(

                const _: () = {
                    #[casper_contract_sdk::linkme::distributed_slice(casper_contract_sdk::abi_generator::ENTRYPOINTS)]
                    #[linkme(crate = casper_contract_sdk::linkme)]
                    static ENTRY_POINTS: fn() -> casper_contract_sdk::schema::SchemaEntryPoint = <#struct_name>::#defs_linkme;
                };
            )*
        }
    }
    #[cfg(not(feature = "__abi_generator"))]
    {
        maybe_abi_collectors = quote! {};
        maybe_entrypoint_defs = quote! {};
    }
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

            #maybe_abi_collectors

            #maybe_entrypoint_defs
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
                    ($($vis:vis $name:ident as $export_name:ident => $dispatch:ident,)*) => {
                        $(
                            $vis fn $name() {
                                #path_to_macro::$dispatch::<#self_ty>();
                            }
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
                    ($($vis:vis $name:ident as $export_name:ident => $dispatch:ident,)*) => {
                        $(
                            #[export_name = stringify!($export_name)]
                            $vis extern "C" fn $name() {
                                #path_to_macro::$dispatch::<#self_ty>();
                            }

                            #[cfg(not(target_arch = "wasm32"))]
                            const _: () = {
                                #[casper_contract_sdk::linkme::distributed_slice(casper_contract_sdk::casper::native::ENTRY_POINTS)]
                                #[linkme(crate = casper_contract_sdk::linkme)]
                                pub static EXPORTS: casper_contract_sdk::casper::native::EntryPoint = casper_contract_sdk::casper::native::EntryPoint {
                                    kind: casper_contract_sdk::casper::native::EntryPointKind::TraitImpl { trait_name: stringify!(#trait_name), impl_name: stringify!(#self_ty), name: stringify!($export_name) },
                                    fptr: || -> () { $name(); },
                                    module_path: module_path!(),
                                    file: file!(),
                                    line: line!(),
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

    let ref_trait = format_ident!("{}Ext", trait_path.require_ident().unwrap());

    let ref_name = format_ident!("{self_ty}Ref");

    code.push(quote! {
        impl #ref_trait for #ref_name {}
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
    // let mut dispatch_table = Vec::new();
    let mut extra_code = Vec::new();
    // let mut schema_entry_points = Vec::new();
    let mut populate_definitions = Vec::new();
    let mut macro_symbols = Vec::new();
    for entry_point in &mut item_trait.items {
        match entry_point {
            syn::TraitItem::Const(_) => todo!("Const"),
            syn::TraitItem::Fn(func) => {
                // let vis  =func.vis;
                let method_attribute = MethodAttribute::from_attributes(&func.attrs).unwrap();
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

                let export_name = if method_attribute.fallback {
                    format_ident!("{}", CASPER_RESERVED_FALLBACK_EXPORT)
                } else {
                    format_ident!("{}", &func_name)
                };

                let _result = match &func.sig.output {
                    syn::ReturnType::Default => {
                        populate_definitions.push(quote! {
                            definitions.populate_one::<()>();
                        });

                        quote! { <() as #crate_path::abi::CasperABI>::declaration() }
                    }
                    syn::ReturnType::Type(_, ty) => {
                        if let Type::Never(_) = ty.as_ref() {
                            populate_definitions.push(quote! {
                                definitions.populate_one::<()>();
                            });

                            quote! { <() as #crate_path::abi::CasperABI>::declaration() }
                        } else {
                            populate_definitions.push(quote! {
                                definitions.populate_one::<#ty>();
                            });

                            quote! { <#ty as #crate_path::abi::CasperABI>::declaration() }
                        }
                    }
                };

                let call_data_return_lifetime = generate_call_data_return(&func.sig.output);

                let dispatch_func_name = format_ident!("{trait_name}_{func_name}_dispatch");

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

                let mut args = Vec::new();
                for (name, ty) in &arg_names_and_types {
                    populate_definitions.push(quote! {
                        definitions.populate_one::<()>();
                    });
                    args.push(quote! {
                        casper_contract_sdk::schema::SchemaArgument {
                            name: stringify!(#name).into(),
                            decl: <#ty as #crate_path::abi::CasperABI>::declaration(),
                        }
                    });
                }

                let flags = EntryPointFlags::empty();

                let _flags = flags.bits();

                let handle_dispatch = match func.sig.inputs.first() {
                    Some(syn::FnArg::Receiver(_receiver)) => {
                        assert!(
                            !method_attribute.private,
                            "can't make dispatcher for private method"
                        );
                        quote! {
                            #vis extern "C" fn #dispatch_func_name<T>()
                            where
                                T: #trait_name
                                    + #crate_path::serializers::borsh::BorshDeserialize
                                    + #crate_path::serializers::borsh::BorshSerialize
                                    + Default
                            {
                                #[derive(#crate_path::serializers::borsh::BorshDeserialize)]
                                #[borsh(crate = #borsh_path)]
                                struct Arguments {
                                    #(#args_attrs,)*
                                }

                                let mut flags = #crate_path::casper_executor_wasm_common::flags::ReturnFlags::empty();
                                let mut instance: T = #crate_path::casper::read_state().unwrap();
                                let input = #crate_path::prelude::casper::copy_input();
                                let args: Arguments = #crate_path::serializers::borsh::from_slice(&input).unwrap();

                                let ret = instance.#func_name(#(args.#arg_names,)*);

                                #crate_path::casper::write_state(&instance).unwrap();

                                let ret_bytes = #crate_path::serializers::borsh::to_vec(&ret).unwrap();
                                #crate_path::casper::ret(flags, Some(&ret_bytes));
                            }
                        }
                    }

                    None | Some(syn::FnArg::Typed(_)) => {
                        assert!(
                            !method_attribute.private,
                            "can't make dispatcher for private static method"
                        );
                        quote! {
                            #vis extern "C"  fn #dispatch_func_name<T: #trait_name>() {
                                #[derive(#crate_path::serializers::borsh::BorshDeserialize)]
                                #[borsh(crate = #borsh_path)]
                                struct Arguments {
                                    #(#args_attrs,)*
                                }


                                let input = #crate_path::prelude::casper::copy_input();
                                let args: Arguments = #crate_path::serializers::borsh::from_slice(&input).unwrap();


                                let _ret = <T as #trait_name>::#func_name(#(args.#arg_names,)*);
                            }
                        }
                    }
                };

                macro_symbols.push(quote! {
                    #vis #func_name as #export_name => #dispatch_func_name
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

                let is_fallback = method_attribute.fallback;

                if !is_fallback {
                    extra_code.push(quote! {
                    fn #func_name<'a>(#self_ty #(#arg_names: #arg_types,)*) -> impl #crate_path::ToCallData<Return<'a> = #call_data_return_lifetime> {
                        #[derive(#crate_path::serializers::borsh::BorshSerialize)]
                        #[borsh(crate = #borsh_path)]
                        struct CallData {
                            #(pub #arg_names: #arg_types,)*
                        }

                        impl #crate_path::ToCallData for CallData {
                            // const SELECTOR: vm_common::selector::Selector = vm_common::selector::Selector::new(#selector_value);

                            type Return<'a> = #call_data_return_lifetime;

                            fn entry_point(&self) -> &str { stringify!(#func_name) }
                            fn input_data(&self) -> Option<Vec<u8>> {
                                #input_data_content
                            }
                        }

                        CallData {
                            #(#arg_names,)*
                        }
                    }
                });
                }
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
    let ref_struct_trait = format_ident!("{trait_name}Ext");

    let macro_name = format_ident!("enumerate_{trait_name}_symbols");

    let maybe_exported_macro = if !trait_meta.export.unwrap_or(false) {
        quote! {
            #[allow(non_snake_case, unused_macros)]
            macro_rules! #macro_name {
                ($mac:ident) => {
                    $mac! {
                        #(#macro_symbols,)*
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
                        #(#macro_symbols,)*
                    }
                }
            }
        }
    };

    let extension_struct = quote! {
        #vis trait #ref_struct_trait: Sized {
            #(#extra_code)*
        }

        #vis struct #ref_struct;

        impl #ref_struct {

        }

        #maybe_exported_macro

        #(#dispatch_functions)*

        // TODO: Rename Ext with Ref, since Ref struct can be pub(crate)'d
        impl #ref_struct_trait for #ref_struct {}
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

fn generate_casper_state_for_struct(
    item_struct: &ItemStruct,
    struct_meta: StructMeta,
) -> impl quote::ToTokens {
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

    quote! {
        #[derive(#crate_path::serializers::borsh::BorshSerialize, #crate_path::serializers::borsh::BorshDeserialize)]
        #[borsh(crate = #borsh_path)]
        #maybe_derive_abi
        #item_struct
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

    quote! {
        #[derive(#crate_path::serializers::borsh::BorshSerialize, #crate_path::serializers::borsh::BorshDeserialize)]
        #[borsh(use_discriminant = true, crate = #borsh_path)]
        #[repr(u32)]
        #maybe_derive_abi
        #item_enum
    }
}

fn get_maybe_derive_abi(_crate_path: impl ToTokens) -> impl ToTokens {
    #[cfg(feature = "__abi_generator")]
    {
        quote! {
            #[derive(#_crate_path::macros::CasperABI)]
        }
    }

    #[cfg(not(feature = "__abi_generator"))]
    {
        quote! {}
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

    // Optionally, generate a schema export if the appropriate flag
    // is set.
    let maybe_casper_schema = {
        #[cfg(feature = "__embed_schema")]
        quote! {
            const SCHEMA: Option<&str> = option_env!("__CARGO_CASPER_INJECT_SCHEMA_MARKER");

            #[no_mangle]
            pub extern "C" fn __casper_schema() {
                use #crate_path::casper::ret;
                use #crate_path::casper_executor_wasm_common::flags::ReturnFlags;
                let bytes = SCHEMA.unwrap_or_default().as_bytes();
                ret(ReturnFlags::empty(), Some(bytes));
            }
        }
        #[cfg(not(feature = "__embed_schema"))]
        quote! {}
    };

    quote! {
        #[derive(#crate_path::serializers::borsh::BorshSerialize, #crate_path::serializers::borsh::BorshDeserialize)]
        #[borsh(crate = #borsh_path)]
        #maybe_derive_abi
        #contract_struct

        #vis struct #ref_name;

        impl #crate_path::ContractRef for #ref_name {
            fn new() -> Self {
                #ref_name
            }
        }

        #maybe_casper_schema
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
        let mut populate_definitions = Vec::new();
        let name = input.ident.clone();
        let mut items = Vec::new();
        for field in &input.fields {
            match &field.ty {
                Type::Path(path) => {
                    for segment in &path.path.segments {
                        let field_name = &field.ident;

                        populate_definitions.push(quote! {
                            definitions.populate_one::<#segment>();
                        });

                        items.push(quote! {
                            casper_contract_sdk::abi::StructField {
                                name: stringify!(#field_name).into(),
                                decl: <#segment>::declaration(),
                            }
                        });
                    }
                }
                other_ty => todo!("Unsupported type {other_ty:?}"),
            }
        }

        Ok(quote! {
            impl casper_contract_sdk::abi::CasperABI for #name {
                fn populate_definitions(definitions: &mut casper_contract_sdk::abi::Definitions) {
                    #(#populate_definitions)*;
                }

                fn declaration() -> casper_contract_sdk::abi::Declaration {
                    const DECL: &str = concat!(module_path!(), "::", stringify!(#name));
                    DECL.into()
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

        let mut all_definitions = Vec::new();
        let mut all_variants = Vec::new();
        let mut populate_definitions = Vec::new();
        let mut has_unit_definition = false;

        // populate_definitions.push(quote! {
        //     definitions.populate_one::<#name>();
        // });

        all_definitions.push(quote! {
            casper_contract_sdk::abi::Definition::Enum {
                name: stringify!(#name).into(),
            }
        });

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
                        populate_definitions.push(quote! {
                            definitions.populate_one::<()>();
                        });
                        has_unit_definition = true;
                    }

                    quote! {
                        <()>::declaration()
                    }
                }
                Fields::Named(named) => {
                    let mut fields = Vec::new();

                    let variant_name = format_ident!("{name}_{variant_name}");

                    for field in &named.named {
                        let field_name = &field.ident;
                        match &field.ty {
                            Type::Path(path) => {
                                populate_definitions.push(quote! {
                                    definitions.populate_one::<#path>();
                                });

                                fields.push(quote! {
                                    casper_contract_sdk::abi::StructField {
                                        name: stringify!(#field_name).into(),
                                        decl: <#path as casper_contract_sdk::abi::CasperABI>::declaration()
                                    }
                                });
                            }
                            other_ty => todo!("Unsupported type {other_ty:?}"),
                        }
                    }

                    populate_definitions.push(quote! {
                        definitions.populate_custom(
                            stringify!(#variant_name).into(),
                            casper_contract_sdk::abi::Definition::Struct {
                                items: vec![
                                    #(#fields,)*
                                ],
                            });
                    });

                    quote! {
                        stringify!(#variant_name).into()
                    }
                }
                Fields::Unnamed(unnamed_fields) => {
                    let mut fields = Vec::new();

                    let variant_name = format_ident!("{name}_{variant_name}");

                    for field in &unnamed_fields.unnamed {
                        match &field.ty {
                            Type::Path(path) => {
                                for segment in &path.path.segments {
                                    let type_name = &segment.ident;
                                    populate_definitions.push(quote! {
                                        definitions.populate_one::<#type_name>();
                                    });

                                    fields.push(quote! {
                                        <#type_name as casper_contract_sdk::abi::CasperABI>::declaration()
                                    });
                                }
                            }
                            other_ty => todo!("Unsupported type {other_ty:?}"),
                        }
                    }

                    populate_definitions.push(quote! {
                        definitions.populate_custom(
                            stringify!(#variant_name).into(),
                            casper_contract_sdk::abi::Definition::Tuple {
                                items: vec![
                                    #(#fields,)*
                                ],
                            });
                    });

                    quote! {
                        stringify!(#variant_name).into()
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
                fn populate_definitions(definitions: &mut casper_contract_sdk::abi::Definitions) {
                    #(#populate_definitions)*;
                }

                fn declaration() -> casper_contract_sdk::abi::Declaration {
                    const DECL: &str = concat!(module_path!(), "::", stringify!(#name));
                    DECL.into()
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
