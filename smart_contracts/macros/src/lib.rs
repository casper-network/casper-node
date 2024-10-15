pub(crate) mod utils;

extern crate proc_macro;

use darling::{ast, FromAttributes, FromMeta};
use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::{format_ident, quote, ToTokens};
use syn::{
    parse_macro_input, Data, DeriveInput, Fields, ItemEnum, ItemFn, ItemImpl, ItemStruct,
    ItemTrait, ItemUnion, LitStr, Type,
};

use casper_sdk::casper_executor_wasm_common::{flags::EntryPointFlags, selector::Selector};
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
    contract_state: bool,
}

#[derive(Debug, FromMeta)]
struct TraitMeta {}

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

#[proc_macro_derive(Contract)]
pub fn derive_casper_contract(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let contract = parse_macro_input!(input as DeriveInput);

    // let contract_attributes = ContractAttributes::from_attributes(&contract.attrs).unwrap();

    let name = &contract.ident;
    let _vis = &contract.vis;

    let _data_struct = match &contract.data {
        Data::Struct(s) => s,
        Data::Enum(_) => todo!("Enum"),
        Data::Union(_) => todo!("Union"),
    };

    let ref_name = format_ident!("{}Ref", name);

    // let mut dynamic_manifest = Vec::new();
    // let mut impl_for_ref = Vec::new();
    // if let Some(traits) = contract_attributes.impl_traits {
    //     for path in traits.iter() {
    //         let ref_trait = format_ident!("{}Ext", path.require_ident().unwrap());
    //         let ref_struct = format_ident!("{}Ref", path.require_ident().unwrap());
    //         dynamic_manifest.push(quote! {
    //             {
    //                 const DISPATCH_TABLE: &[casper_sdk::sys::EntryPoint] =
    // &(<#ref_struct>::__casper_new_trait_dispatch_table::<#name>());
    // DISPATCH_TABLE             }
    //         });

    //         impl_for_ref.push(quote! {
    //             impl #ref_trait for #ref_name {}
    //         })
    //     }
    // }

    // let ext_struct_name = format_ident!("{st_name}Ref");

    let f = quote! {

        // impl casper_sdk::Contract for #name {
        //     type Ref = #ref_name;

        //     fn name() -> &'static str {
        //         stringify!(#name)
        //     }


        //     fn create<T:casper_sdk::ToCallData>(value: u64, call_data: T) -> Result<casper_sdk::ContractHandle<Self::Ref>, casper_sdk::types::CallError> {
        //         // TODO: Pass multiple manifests by ptr to avoid allocation
        //         let entry_points = &<(#ref_name)>::ENTRY_POINTS;
        //         let entry_points_allocated: Vec<casper_sdk::sys::EntryPoint> = entry_points.into_iter().map(|i| i.into_iter().copied()).flatten().collect();

        //         let manifest = casper_sdk::sys::Manifest {
        //             entry_points: entry_points_allocated.as_ptr(),
        //             entry_points_size: entry_points_allocated.len(),
        //         };

        //         let input_data = call_data.input_data();

        //         let create_result = casper_sdk::host::casper_create(None, &manifest, value, Some(T::SELECTOR), input_data.as_ref().map(|v| v.as_slice()))?;
        //         Ok(casper_sdk::ContractHandle::<Self::Ref>::from_address(create_result.contract_address))
        //     }

        //     fn default_create() -> Result<casper_sdk::ContractHandle<Self::Ref>, casper_sdk::types::CallError> {
        //         // TODO: Pass multiple manifests by ptr to avoid allocation
        //         let entry_points = &<(#ref_name)>::ENTRY_POINTS;
        //         let entry_points_allocated: Vec<casper_sdk::sys::EntryPoint> = entry_points.into_iter().map(|i| i.into_iter().copied()).flatten().collect();
        //         let manifest = casper_sdk::sys::Manifest {
        //             entry_points: entry_points_allocated.as_ptr(),
        //             entry_points_size: entry_points_allocated.len(),
        //         };
        //         let create_result = casper_sdk::host::casper_create(None, &manifest, 0, None, None)?;
        //         Ok(casper_sdk::ContractHandle::<Self::Ref>::from_address(create_result.contract_address))
        //         // todo!()
        //     }

        //     fn upgrade<T: casper_sdk::ToCallData>(
        //         new_code: Option<&[u8]>,
        //         // new_manifest: &[u8],
        //         call_data: T,
        //     ) -> Result<(), casper_sdk::types::CallError> {
        //         // let entry_points = &<(#ref_name)>::ENTRY_POINTS;
        //         // let entry_points_allocated: Vec<casper_sdk::sys::EntryPoint> = entry_points.into_iter().map(|i| i.into_iter().copied()).flatten().collect();
        //     // let manifest = casper_sdk::sys::Manifest {
        //         //     entry_points: entry_points_allocated.as_ptr(),
        //         //     entry_points_size: entry_points_allocated.len(),
        //         // }; let input_data = call_data.input_data();
        //         // let create_result = casper_sdk::host::casper_upgrade(code, &manifest, Some(T::SELECTOR), input_data.as_ref().map(|v| v.as_slice()))?;
        //         Ok(())
        //     }
        // }

        #[derive(Debug)]
        pub struct #ref_name;

        #[doc(hidden)]
        impl #ref_name {
            #[doc(hidden)]
            const ENTRY_POINTS: &'static [&'static [casper_sdk::sys::EntryPoint]] = &[
                // #(#dynamic_manifest,)*
                &(#name::MANIFEST),
            ];
        }

        const _: () = {
            assert!(casper_sdk::sys::utils::fallback_selector_count(<#ref_name>::ENTRY_POINTS) <= 1, "There can be at most one fallback entry point");
        };

        // #(#impl_for_ref)*
    };
    f.into()
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
                quote! { <<#new_ref as core::ops::Deref>::Target as casper_sdk::prelude::borrow::ToOwned>::Owned }
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
    let attrs2 = attrs.clone();
    let attr_args = match ast::NestedMeta::parse_meta_list(attrs2.into()) {
        Ok(v) => v,
        Err(e) => {
            return TokenStream::from(e.to_compile_error());
        }
    };

    let mut has_fallback_selector = false;

    if let Ok(item_struct) = syn::parse::<ItemStruct>(item.clone()) {
        let struct_meta = StructMeta::from_list(&attr_args).unwrap();
        if !struct_meta.contract_state {
            let partial = generate_casper_state_for_struct(item_struct);
            quote! {
                #partial
            }
            .into()
        } else {
            process_casper_contract_for_struct(item_struct)
        }
    } else if let Ok(item_enum) = syn::parse::<ItemEnum>(item.clone()) {
        let partial = generate_casper_state_for_enum(item_enum);
        quote! {
            #partial
        }
        .into()
    } else if let Ok(item_trait) = syn::parse::<ItemTrait>(item.clone()) {
        let trait_meta = TraitMeta::from_list(&attr_args).unwrap();
        casper_trait_definition(item_trait, trait_meta, &mut has_fallback_selector)
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
            ItemFnMeta::Export => generate_export_function(func),
        }
    } else {
        let err = syn::Error::new(
            Span::call_site(),
            "State attribute can only be applied to struct or enum",
        );
        TokenStream::from(err.to_compile_error())
    }

    //     proc_macro::TokenTree::Ident(ident) if ident.to_string() == "export" =>
    //  {

    //         return token.into();
    //     }
    //     other => todo!("other attribute {other:?}"),
    // }
    // }
    // todo!("???")
}

fn generate_export_function(func: ItemFn) -> TokenStream {
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
                casper_sdk::set_panic_hook();
            }

            #func

            #[derive(casper_sdk::serializers::borsh::BorshDeserialize)]
            #[borsh(crate = "casper_sdk::serializers::borsh")]
            struct Arguments {
                #(#args_attrs,)*
            }

            let input = casper_sdk::host::casper_copy_input();
            let args: Arguments = casper_sdk::serializers::borsh::from_slice(&input).unwrap();

            let _ret = #func_name(#(args.#arg_names,)*);
        }

        #[cfg(not(target_arch = "wasm32"))]
        #func

        #[cfg(not(target_arch = "wasm32"))]
        const _: () = {
            // fn #func_name() {
            //     #[derive(casper_sdk::serializers::borsh::BorshDeserialize)]
            //     #[borsh(crate = "casper_sdk::serializers::borsh")]
            //     struct Arguments {
            //         #(#args_attrs,)*
            //     }


            //     let input = casper_sdk::host::casper_copy_input();
            //     let args: Arguments = casper_sdk::serializers::borsh::from_slice(&input).unwrap();

            //     let _ret = #exported_func_name(#(args.#arg_names,)*);
            // }

            #[casper_sdk::linkme::distributed_slice(casper_sdk::host::native::private_exports::EXPORTS)]
            #[linkme(crate = casper_sdk::linkme)]
            pub static EXPORTS: casper_sdk::host::native::Export = casper_sdk::host::native::Export {
                kind: casper_sdk::host::native::ExportKind::Function { name: stringify!(#func_name) },
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
    let mut combined_selectors = Selector::zero();
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
                                casper_sdk::host::write_state(&instance).unwrap();
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
                                casper_sdk::host::write_state(&_ret).unwrap();
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
                        _ => {
                            // There is a return value so call casper_return.
                            Some(quote! {
                                let ret_bytes = casper_sdk::serializers::borsh::to_vec(&_ret).unwrap();
                                casper_sdk::host::casper_return(flags, Some(&ret_bytes));
                            })
                        }
                    }
                };

                assert_eq!(arg_names.len(), arg_types.len());

                let mut prelude = Vec::new();

                prelude.push(quote! {
                    #[derive(casper_sdk::serializers::borsh::BorshDeserialize)]
                    #[borsh(crate = "casper_sdk::serializers::borsh")]
                    struct Arguments {
                        #(#arg_attrs,)*
                    }


                    let input = casper_sdk::host::casper_copy_input();
                    let args: Arguments = casper_sdk::serializers::borsh::from_slice(&input).unwrap();
                });

                if method_attribute.constructor {
                    prelude.push(quote! {
                        if casper_sdk::host::has_state().unwrap() {
                            panic!("State of the contract is already present; unable to proceed with the constructor");
                        }
                    });
                }

                if !method_attribute.payable {
                    let panic_msg = format!(
                        r#"Entry point "{func_name}" is not payable and does not accept tokens"#
                    );
                    prelude.push(quote! {
                        if casper_sdk::host::get_value() != 0 {
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
                            flags |= casper_sdk::casper_executor_wasm_common::flags::ReturnFlags::REVERT;
                        }

                    }
                } else {
                    quote! {}
                };

                let handle_call = if entry_point_requires_state {
                    quote! {
                        let mut instance: #struct_name = casper_sdk::host::read_state().unwrap();
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
                };

                let _bits = flag_value.bits();
                // let _selector_value = selector_value.map(|value|
                // value.get()).unwrap_or_default();

                // #[cfg(feature = "__abi_generator")]
                // abi_generator_entry_points.push(quote! {
                //     #()
                //     const _: () = {
                //         #[casper_sdk::linkme::distributed_slice(casper_sdk::abi_generator::ENTRYPOINTS)]
                //         #[linkme(crate = casper_sdk::linkme)]
                //         static ENTRY_POINTS: fn() -> casper_sdk::schema::SchemaEntryPoint =
                // <#struct_name>::__casper_entry_points;     };
                // });

                let extern_func_name = format_ident!("__casper_export_{func_name}");

                extern_entry_points.push(quote! {

                    #[export_name = stringify!(#export_name)]
                    #vis extern "C" fn #extern_func_name() {
                        // Set panic hook (assumes std is enabled etc.)
                        #[cfg(target_arch = "wasm32")]
                        {
                            casper_sdk::set_panic_hook();
                        }

                        #(#prelude;)*

                        let mut flags = casper_sdk::casper_executor_wasm_common::flags::ReturnFlags::empty();

                        #handle_call;

                        #handle_err;

                        #handle_write_state;

                        #handle_ret;
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    const _: () = {
                        #[casper_sdk::linkme::distributed_slice(casper_sdk::host::native::private_exports::EXPORTS)]
                        #[linkme(crate = casper_sdk::linkme)]
                        pub static EXPORTS: casper_sdk::host::native::Export = casper_sdk::host::native::Export {
                            kind: casper_sdk::host::native::ExportKind::SmartContract { name: stringify!(#export_name), struct_name: stringify!(#struct_name) },
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
                                Some(casper_sdk::serializers::borsh::to_vec(&self).expect("Serialization to succeed"))
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
                                        pub fn #func_name<'a>(#self_ty #(#arg_names: #arg_types,)*) -> impl casper_sdk::ToCallData<Return<'a> = #call_data_return_lifetime> {
                                            #[derive(casper_sdk::serializers::borsh::BorshSerialize, PartialEq, Debug)]
                                            #[borsh(crate = "casper_sdk::serializers::borsh")]
                                            struct #ident {
                                                #(#arg_names: #arg_types,)*
                                            }

                                            impl casper_sdk::ToCallData for #ident {
                                                // const SELECTOR: vm_common::selector::Selector = vm_common::selector::Selector::new(#selector_value);

                                                type Return<'a> = #call_data_return_lifetime;

                                                fn entry_point(&self) -> &str { stringify!(#func_name) }

                                                fn input_data(&self) -> Option<casper_sdk::serializers::borsh::__private::maybestd::vec::Vec<u8>> {
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
                casper_sdk::schema::SchemaArgument {
                    name: stringify!(#name).into(),
                    decl: <#ty as casper_sdk::abi::CasperABI>::declaration(),
                }
            });
        }

        // let mut args = Vec::new();
        // for arg in &entry_point

        let selector_value = utils::compute_selector_value(&func.sig);
        combined_selectors ^= Selector::from(selector_value);

        #[cfg(feature = "__abi_generator")]
        {
            let bits = flag_value.bits();

            let schema_selector = 0u32; // TODO: Probably wise to remove this from schema for now.
            let result = match &func.sig.output {
                syn::ReturnType::Default => {
                    populate_definitions.push(quote! {
                        definitions.populate_one::<()>();
                    });

                    quote! { <() as casper_sdk::abi::CasperABI>::declaration() }
                }
                syn::ReturnType::Type(_, ty) => match ty.as_ref() {
                    Type::Never(_) => {
                        populate_definitions.push(quote! {
                            definitions.populate_one::<()>();
                        });

                        quote! { <() as casper_sdk::abi::CasperABI>::declaration() }
                    }
                    _ => {
                        populate_definitions.push(quote! {
                            definitions.populate_one::<#ty>();
                        });

                        quote! { <#ty as casper_sdk::abi::CasperABI>::declaration() }
                    }
                },
            };

            let func_name = &func.sig.ident;

            let linkme_schema_entry_point_ident =
                format_ident!("__casper_schema_entry_point_{func_name}");

            defs.push(quote! {
                fn #linkme_schema_entry_point_ident() -> casper_sdk::schema::SchemaEntryPoint {
                    casper_sdk::schema::SchemaEntryPoint {
                        name: stringify!(#func_name).into(),
                        selector: Some(#schema_selector),
                        arguments: vec![ #(#args,)* ],
                        result: #result,
                        flags: casper_sdk::casper_executor_wasm_common::flags::EntryPointFlags::from_bits(#bits).unwrap(),
                    }
                }
            });
            defs_linkme.push(linkme_schema_entry_point_ident);

            let linkme_abi_populate_defs_ident =
                format_ident!("__casper_populate_definitions_{func_name}");

            defs.push(quote! {
                fn #linkme_abi_populate_defs_ident(definitions: &mut casper_sdk::abi::Definitions) {
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
                    #[casper_sdk::linkme::distributed_slice(casper_sdk::abi_generator::ABI_COLLECTORS)]
                    #[linkme(crate = casper_sdk::linkme)]
                    static COLLECTOR: fn(&mut casper_sdk::abi::Definitions) = <#struct_name>::#populate_definitions_linkme;
                };
            )*
        };

        maybe_entrypoint_defs = quote! {
            #(
                const _: () = {
                    #[casper_sdk::linkme::distributed_slice(casper_sdk::abi_generator::ENTRYPOINTS)]
                    #[linkme(crate = casper_sdk::linkme)]
                    static ENTRY_POINTS: fn() -> casper_sdk::schema::SchemaEntryPoint = <#struct_name>::#defs_linkme;
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
                // #[doc(hidden)]
                // fn __casper_populate_definitions(definitions: &mut casper_sdk::abi::Definitions) {
                //     <#struct_name as casper_sdk::abi::CasperABI>::populate_definitions(definitions);
                //     #(#populate_definitions)*;
                // }

                // #[doc(hidden)]
                // fn __casper_schema() -> casper_sdk::schema::Schema {
                //     const VERSION: &str = env!("CARGO_PKG_VERSION");

                //     let mut entry_points = Self::__casper_entry_points();

                //     let definitions = {
                //         let mut definitions = casper_sdk::abi::Definitions::default();
                //         Self::__casper_populate_definitions(&mut definitions);
                //         definitions
                //     };

                //     let state = <#struct_name as casper_sdk::abi::CasperABI>::declaration();
                //     casper_sdk::schema::Schema {
                //         name: stringify!(#struct_name).into(),
                //         type_: casper_sdk::schema::SchemaType::Contract {
                //             state,
                //         },
                //         version: Some(VERSION.into()),
                //         definitions,
                //         entry_points,
                //     }
                // }

                // #[doc(hidden)]
                // const MANIFEST: [casper_sdk::sys::EntryPoint; #entry_points_len] = [#(#entry_points,)*];

                #(#defs)*


            }

            #maybe_abi_collectors

            #maybe_entrypoint_defs
            #(#extern_entry_points)*

        }),
    };
    let ref_struct_name = format_ident!("{st_name}Ref");
    let _combined_selectors = combined_selectors.get();
    quote! {
        #entry_points

        #handle_manifest

        impl #ref_struct_name {
            // TODO: Collect combined selectors
            // pub const SELECTOR: vm_common::selector::Selector = vm_common::selector::Selector::new(#combined_selectors);

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
    let trait_name = quote! { #trait_path };
    let macro_name = format_ident!("enumerate_{trait_name}_symbols");

    let path_to_macro = match impl_meta.path {
        Some(path) => quote! { #path },
        None => {
            quote! { self }
        }
    };

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

                #path_to_macro::#macro_name!(visitor);
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
                                #[casper_sdk::linkme::distributed_slice(casper_sdk::host::native::private_exports::EXPORTS)]
                                #[linkme(crate = casper_sdk::linkme)]
                                pub static EXPORTS: casper_sdk::host::native::Export = casper_sdk::host::native::Export {
                                    kind: casper_sdk::host::native::ExportKind::TraitImpl { trait_name: stringify!(#trait_name), impl_name: stringify!(#self_ty), name: stringify!($export_name) },
                                    fptr: || -> () { $name(); },
                                    module_path: module_path!(),
                                    file: file!(),
                                    line: line!(),
                                };
                            };
                        )*
                    }
                }

                #path_to_macro::#macro_name!(visitor);
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

fn casper_trait_definition(
    mut item_trait: ItemTrait,
    _trait_meta: TraitMeta,
    _has_fallback_selector: &mut bool,
) -> TokenStream {
    let mut combined_selectors = Selector::zero();
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

                        quote! { <() as casper_sdk::abi::CasperABI>::declaration() }
                    }
                    syn::ReturnType::Type(_, ty) => match ty.as_ref() {
                        Type::Never(_) => {
                            populate_definitions.push(quote! {
                                definitions.populate_one::<()>();
                            });

                            quote! { <() as casper_sdk::abi::CasperABI>::declaration() }
                        }
                        _ => {
                            populate_definitions.push(quote! {
                                definitions.populate_one::<#ty>();
                            });

                            quote! { <#ty as casper_sdk::abi::CasperABI>::declaration() }
                        }
                    },
                };

                let call_data_return_lifetime = generate_call_data_return(&func.sig.output);

                let dispatch_func_name = format_ident!("{trait_name}_{func_name}_dispatch");

                let selector_value = utils::compute_selector_value(&func.sig);
                combined_selectors ^= Selector::from(selector_value);

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
                        casper_sdk::schema::SchemaArgument {
                            name: stringify!(#name).into(),
                            decl: <#ty as casper_sdk::abi::CasperABI>::declaration(),
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
                            #vis extern "C" fn #dispatch_func_name<T: #trait_name + casper_sdk::serializers::borsh::BorshDeserialize + casper_sdk::serializers::borsh::BorshSerialize + Default>() {
                                #[derive(casper_sdk::serializers::borsh::BorshDeserialize)]
                                #[borsh(crate = "casper_sdk::serializers::borsh")]
                                struct Arguments {
                                    #(#args_attrs,)*
                                }

                                let mut flags = casper_sdk::casper_executor_wasm_common::flags::ReturnFlags::empty();
                                let mut instance: T = casper_sdk::host::read_state().unwrap();
                                let input = casper_sdk::host::casper_copy_input();
                                let args: Arguments = casper_sdk::serializers::borsh::from_slice(&input).unwrap();

                                let ret = instance.#func_name(#(args.#arg_names,)*);

                                casper_sdk::host::write_state(&instance).unwrap();

                                let ret_bytes = casper_sdk::serializers::borsh::to_vec(&ret).unwrap();
                                casper_sdk::host::casper_return(flags, Some(&ret_bytes));
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
                                #[derive(casper_sdk::serializers::borsh::BorshDeserialize)]
                                #[borsh(crate = "casper_sdk::serializers::borsh")]
                                struct Arguments {
                                    #(#args_attrs,)*
                                }


                                let input = casper_sdk::host::casper_copy_input();
                                let args: Arguments = casper_sdk::serializers::borsh::from_slice(&input).unwrap();


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
                        Some(casper_sdk::serializers::borsh::to_vec(&self).expect("Serialization to succeed"))
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
                    fn #func_name<'a>(#self_ty #(#arg_names: #arg_types,)*) -> impl casper_sdk::ToCallData<Return<'a> = #call_data_return_lifetime> {
                        #[derive(casper_sdk::serializers::borsh::BorshSerialize)]
                        #[borsh(crate = "casper_sdk::serializers::borsh")]
                        struct CallData {
                            #(pub #arg_names: #arg_types,)*
                        }

                        impl casper_sdk::ToCallData for CallData {
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
            syn::TraitItem::Type(_) => todo!("Type"),
            syn::TraitItem::Macro(_) => todo!("Macro"),
            syn::TraitItem::Verbatim(_) => todo!("Verbatim"),
            other => todo!("Other {other:?}"),
        }
    }
    let ref_struct = format_ident!("{trait_name}Ref");
    let ref_struct_trait = format_ident!("{trait_name}Ext");

    let combined_selectors = combined_selectors.get();
    let macro_name = format_ident!("enumerate_{trait_name}_symbols");

    let extension_struct = quote! {
        #vis trait #ref_struct_trait: Sized {
            #(#extra_code)*
        }

        #vis struct #ref_struct;

        impl #ref_struct {
            pub const SELECTOR: casper_sdk::casper_executor_wasm_common::selector::Selector = casper_sdk::casper_executor_wasm_common::selector::Selector::new(#combined_selectors);

            // #[doc(hidden)]
            // #vis const fn __casper_new_trait_dispatch_table<T: #trait_name + borsh::BorshDeserialize + borsh::BorshSerialize + Default>() -> [casper_sdk::sys::EntryPoint; #manifest_data_len] {
            //     // This will create set of extern "C" function pointers that will dispatch to a concerete implementation.
            //     // Essentially, this constructor provides 'late binding' and crates function pointers for any concrete implementation of given trait.
            //     [
            //         #(#dispatch_table,)*
            //     ]
            // }
            // // #[doc(hidden)]
            // #vis fn __casper_populate_definitions(definitions: &mut casper_sdk::abi::Definitions) {
            //     #(#populate_definitions)*;
            // }

            // #[doc(hidden)]
            // #vis fn __casper_schema_entry_points() -> Vec<casper_sdk::schema::SchemaEntryPoint> {
            //     vec![
            //         #(#schema_entry_points,)*
            //     ]
            // }
        }




        // #[macro_export]
        // mod #mod_name {
        #[allow(non_snake_case, unused_macros)]
        macro_rules! #macro_name {
            ($mac:ident) => {
                $mac! {
                    #(#macro_symbols,)*
                }
            }
        }

        pub(crate) use #macro_name;

        #(#dispatch_functions)*

        // }
        // }

        // pub(crate) use #mod_name::#macro_name;

        // TODO: Rename Ext with Ref, since Ref struct can be pub(crate)'d
        impl #ref_struct_trait for #ref_struct {}

            // impl casper_sdk::schema::CasperSchema for #ref_struct {
            //     fn schema() -> casper_sdk::schema::Schema {
            //         const VERSION: &str = env!("CARGO_PKG_VERSION");

            //         let entry_points = Self::__casper_schema_entry_points();

            //         let definitions = {
            //             let mut definitions = casper_sdk::abi::Definitions::default();
            //             Self::__casper_populate_definitions(&mut definitions);
            //             definitions
            //         };

            //         casper_sdk::schema::Schema {
            //             name: stringify!(#trait_name).into(),
            //             version: Some(VERSION.into()),
            //             type_: casper_sdk::schema::SchemaType::Interface,
            //             definitions,
            //             entry_points,
            //         }
            //     }
            // }

            impl casper_sdk::ContractRef for #ref_struct {
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

fn generate_casper_state_for_struct(item_struct: ItemStruct) -> impl quote::ToTokens {
    let maybe_derive_abi = get_maybe_derive_abi();

    quote! {
        #[derive(casper_sdk::serializers::borsh::BorshSerialize, casper_sdk::serializers::borsh::BorshDeserialize)]
        #[borsh(crate = "casper_sdk::serializers::borsh")]
        #maybe_derive_abi
        #item_struct
    }
}

fn generate_casper_state_for_enum(item_enum: ItemEnum) -> impl quote::ToTokens {
    let maybe_derive_abi = get_maybe_derive_abi();

    quote! {
        #[derive(casper_sdk::serializers::borsh::BorshSerialize, casper_sdk::serializers::borsh::BorshDeserialize)]
        #[borsh(use_discriminant = true, crate = "casper_sdk::serializers::borsh")]
        #[repr(u32)]
        #maybe_derive_abi
        #item_enum
    }
}

fn get_maybe_derive_abi() -> impl ToTokens {
    #[cfg(feature = "__abi_generator")]
    {
        quote! {
            #[derive(casper_macros::CasperABI)]
        }
    }

    #[cfg(not(feature = "__abi_generator"))]
    {
        quote! {}
    }
}

fn process_casper_contract_for_struct(contract_struct: ItemStruct) -> TokenStream {
    let struct_name = &contract_struct.ident;
    let ref_name = format_ident!("{struct_name}Ref");
    let vis = &contract_struct.vis;

    let maybe_derive_abi = get_maybe_derive_abi();

    quote! {
        #[derive(casper_sdk::serializers::borsh::BorshSerialize, casper_sdk::serializers::borsh::BorshDeserialize)]
        #[borsh(crate = "casper_sdk::serializers::borsh")]
        #maybe_derive_abi
        #contract_struct

        #vis struct #ref_name;

        impl casper_sdk::ContractRef for #ref_name {
            fn new() -> Self {
                #ref_name
            }
        }
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
            let #typed = casper_sdk::get_named_arg(stringify!(#name)).expect("should get named arg");
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

            // casper_sdk::EntryPoint {
            //     name: #func_name,
            //     params: &[
            //         #(#params,)*
            //     ],
            //     func: closure,
            // }

            result.expect("should work")
        }
    };

    println!("{}", gen);

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
//         impl casper_sdk::schema::CasperSchema for #name {
//             fn schema() -> casper_sdk::schema::Schema {
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
        for field in input.fields.iter() {
            match &field.ty {
                Type::Path(path) => {
                    for segment in &path.path.segments {
                        let field_name = &field.ident;

                        populate_definitions.push(quote! {
                            definitions.populate_one::<#segment>();
                        });

                        items.push(quote! {
                            casper_sdk::abi::StructField {
                                name: stringify!(#field_name).into(),
                                decl: <#segment>::declaration(),
                            }
                        })
                    }
                }
                other_ty => todo!("Unsupported type {other_ty:?}"),
            }
        }

        Ok(quote! {
            impl casper_sdk::abi::CasperABI for #name {
                fn populate_definitions(definitions: &mut casper_sdk::abi::Definitions) {
                    #(#populate_definitions)*;
                }

                fn declaration() -> casper_sdk::abi::Declaration {
                    format!("{}::{}", module_path!(), stringify!(#name))
                }

                fn definition() -> casper_sdk::abi::Definition {
                    casper_sdk::abi::Definition::Struct {
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
            casper_sdk::abi::Definition::Enum {
                name: stringify!(#name).into(),
            }
        });

        let mut current_discriminant = 0;

        for variant in input.variants.iter() {
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
                                    casper_sdk::abi::StructField {
                                        name: stringify!(#field_name).into(),
                                        decl: <#path as casper_sdk::abi::CasperABI>::declaration()
                                    }
                                });
                            }
                            other_ty => todo!("Unsupported type {other_ty:?}"),
                        }
                    }

                    populate_definitions.push(quote! {
                        definitions.populate_custom(
                            stringify!(#variant_name).into(),
                            casper_sdk::abi::Definition::Struct {
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
                                        <#type_name as casper_sdk::abi::CasperABI>::declaration()
                                    });
                                }
                            }
                            other_ty => todo!("Unsupported type {other_ty:?}"),
                        }
                    }

                    populate_definitions.push(quote! {
                        definitions.populate_custom(
                            stringify!(#variant_name).into(),
                            casper_sdk::abi::Definition::Tuple {
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
                casper_sdk::abi::EnumVariant {
                    name: stringify!(#variant_name).into(),
                    discriminant: #current_discriminant,
                    decl: #variant_decl,
                }
            });

            current_discriminant += 1;
        }

        Ok(quote! {
            impl casper_sdk::abi::CasperABI for #name {
                fn populate_definitions(definitions: &mut casper_sdk::abi::Definitions) {
                    #(#populate_definitions)*;
                }

                fn declaration() -> casper_sdk::abi::Declaration {
                    format!("{}::{}", module_path!(), stringify!(#name))
                }

                fn definition() -> casper_sdk::abi::Definition {
                    casper_sdk::abi::Definition::Enum {
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

/// Procedural macro that computes a selector for a given byte literal.
#[proc_macro]
pub fn selector(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as LitStr);

    let str = input.value();
    let bytes = str.as_bytes();

    let selector_value = utils::compute_selector_bytes(bytes);

    TokenStream::from(quote! {
        casper_sdk::casper_executor_wasm_common::selector::Selector::new(#selector_value)
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
