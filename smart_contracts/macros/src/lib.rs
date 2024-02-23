extern crate proc_macro;

use blake2_rfc::blake2b;
use casper_sdk::Selector;
use darling::{usage::UsesLifetimes, FromAttributes, FromDeriveInput, FromMeta};
use proc_macro::{Literal, TokenStream, TokenTree};
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{
    parse_macro_input, token::Struct, Data, DeriveInput, Error, Fields, ItemEnum, ItemFn, ItemImpl,
    ItemStruct, ItemTrait, ItemUnion, Lit, LitByteStr, LitStr, Meta, Path, Type,
};
use vm_common::flags::{self, EntryPointFlags};

#[derive(Debug, FromAttributes)]
#[darling(attributes(casper))]
struct MethodAttribute {
    #[darling(default)]
    constructor: bool,
    #[darling(default)]
    revert_on_error: bool,
    /// Explicitly mark method as private so it's not externally callable.
    #[darling(default)]
    private: bool,
}

#[derive(Debug, FromAttributes)]
#[darling(attributes(casper))]
struct ContractAttributes {
    impl_traits: Option<darling::util::PathList>,
}

#[proc_macro_derive(Contract)]
pub fn derive_casper_contract(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    let contract = parse_macro_input!(input as DeriveInput);

    let contract_attributes = ContractAttributes::from_attributes(&contract.attrs).unwrap();

    let name = &contract.ident;
    let _vis = &contract.vis;

    let data_struct = match &contract.data {
        Data::Struct(s) => s,
        Data::Enum(_) => todo!("Enum"),
        Data::Union(_) => todo!("Union"),
    };

    // let manifest_name = format_ident!("{name}::MANIFEST");
    let ref_name = format_ident!("{}Ref", name);

    let mut dynamic_manifest = Vec::new();
    let mut impl_for_ref = Vec::new();
    if let Some(traits) = contract_attributes.impl_traits {
        for path in traits.iter() {
            let ref_trait = format_ident!("{}Ext", path.require_ident().unwrap());
            let ref_struct = format_ident!("{}Ref", path.require_ident().unwrap());
            dynamic_manifest.push(quote! {
                {
                    const DISPATCHER: &[casper_sdk::sys::EntryPoint] = &(<#ref_struct>::__casper_new_trait_dispatch_table::<#name>());
                    DISPATCHER
                }
            });

            impl_for_ref.push(quote! {
                impl #ref_trait for #ref_name {}
            })
        }
    }

    // let ext_struct_name = format_ident!("{st_name}Ref");

    let f = quote! {

        impl casper_sdk::Contract for #name {
            type Ref = #ref_name;

            fn name() -> &'static str {
                stringify!(#name)
            }

            fn create<T:casper_sdk::ToCallData>(call_data: T) -> Result<casper_sdk::ContractHandle<Self::Ref>, casper_sdk::types::CallError> {
                let entry_points: &[&[casper_sdk::sys::EntryPoint]] = &[
                    #(#dynamic_manifest,)*
                    #name::MANIFEST.as_slice(),
                ];


                // TODO: Pass multiple manifests by ptr to avoid allocation
                let entry_points_allocated: Vec<casper_sdk::sys::EntryPoint> = entry_points.into_iter().map(|i| i.into_iter().copied()).flatten().collect();

                let manifest = casper_sdk::sys::Manifest {
                    entry_points: entry_points_allocated.as_ptr(),
                    entry_points_size: entry_points_allocated.len(),
                };

                let input_data = call_data.input_data();

                let create_result = casper_sdk::host::casper_create(None, &manifest, Some(T::SELECTOR), input_data.as_ref().map(|v| v.as_slice()))?;
                Ok(casper_sdk::ContractHandle::<Self::Ref>::from_address(create_result.contract_address))
            }

            fn default_create() -> Result<casper_sdk::ContractHandle<Self::Ref>, casper_sdk::types::CallError> {
                let entry_points: &[&[casper_sdk::sys::EntryPoint]] = &[
                    #(#dynamic_manifest,)*
                    #name::MANIFEST.as_slice(),
                ];


                // TODO: Pass multiple manifests by ptr to avoid allocation
                let entry_points_allocated: Vec<casper_sdk::sys::EntryPoint> = entry_points.into_iter().map(|i| i.into_iter().copied()).flatten().collect();
                let manifest = casper_sdk::sys::Manifest {
                    entry_points: entry_points_allocated.as_ptr(),
                    entry_points_size: entry_points_allocated.len(),
                };
                let create_result = casper_sdk::host::casper_create(None, &manifest, None, None)?;
                Ok(casper_sdk::ContractHandle::<Self::Ref>::from_address(create_result.contract_address))
            }
        }

        #[derive(Debug)]
        pub struct #ref_name;

        #(#impl_for_ref)*
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
                quote! { <<#new_ref as core::ops::Deref>::Target as alloc::borrow::ToOwned>::Owned }
            }
            _ => {
                quote! { #ty }
            }
        },
    }
}

#[proc_macro_attribute]
pub fn casper(attrs: TokenStream, item: TokenStream) -> TokenStream {
    let mut attrs_iter = attrs.into_iter().peekable();

    for attr in &mut attrs_iter {
        let item = item.clone();
        match attr {
            proc_macro::TokenTree::Ident(ident) if ident.to_string() == "trait_definition" => {
                let mut item_trait = parse_macro_input!(item as ItemTrait);
                // todo!("{item_trait:#?}");
                let trait_name = &item_trait.ident;

                let vis = &item_trait.vis;

                // let mut manifest_data = Vec::new();

                let mut dispatch_table = Vec::new();

                let mut entry_point_index = 0usize;
                let mut extra_code = Vec::new();

                let mut schema_entry_points = Vec::new();

                let mut populate_definitions = Vec::new();

                for entry_point in &mut item_trait.items {
                    match entry_point {
                        syn::TraitItem::Const(_) => todo!("Const"),
                        syn::TraitItem::Fn(func) => {
                            let method_attribute =
                                MethodAttribute::from_attributes(&func.attrs).unwrap();
                            func.attrs.clear();
                            if method_attribute.private {
                                continue;
                            }

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

                            let call_data_return_lifetime =
                                generate_call_data_return(&func.sig.output);
                            // let call_data

                            let func_name = func.sig.ident.clone();
                            let name_str = func_name.to_string();

                            let dispatch_func_name =
                                format_ident!("{trait_name}_{func_name}_dispatch");

                            let selector = compute_selector(name_str.as_bytes()).get();

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
                            let arg_types: Vec<_> =
                                arg_names_and_types.iter().map(|(_name, ty)| ty).collect();

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

                            schema_entry_points.push(quote! {
                                casper_sdk::schema::SchemaEntryPoint {
                                    name: stringify!(#func_name).into(),
                                    selector: #selector,
                                    arguments: vec![ #(#args,)* ],
                                    result: #result,
                                    flags: vm_common::flags::EntryPointFlags::default(), // TODO: Constructors in traits
                                }
                            });

                            let handle_dispatch = match func.sig.inputs.first() {
                                Some(syn::FnArg::Receiver(_receiver)) => {
                                    quote! {
                                        extern "C" fn #dispatch_func_name<T: #trait_name + borsh::BorshDeserialize + borsh::BorshSerialize + casper_sdk::Contract + Default>() {
                                            let mut flags = vm_common::flags::ReturnFlags::empty();

                                            let mut instance: T = casper_sdk::host::read_state().unwrap();
                                            let ret = casper_sdk::host::start_noret(|(#(#arg_names,)*):(#(#arg_types,)*)| {
                                                instance.#func_name(#(#arg_names,)*)
                                            });

                                            casper_sdk::host::write_state(&instance).unwrap();
                                            let ret_bytes = borsh::to_vec(&ret).unwrap();
                                            casper_sdk::host::casper_return(flags, Some(&ret_bytes));
                                        }
                                    }
                                }

                                None | Some(syn::FnArg::Typed(_)) => {
                                    quote! {
                                        extern "C" fn #dispatch_func_name<T: #trait_name + borsh::BorshDeserialize + borsh::BorshSerialize + casper_sdk::Contract + Default>() {
                                            let _ret = casper_sdk::host::start(|(#(#arg_names,)*):(#(#arg_types,)*)| {
                                                <T as #trait_name>::#func_name(#(#arg_names,)*)
                                            });
                                        }
                                    }
                                }
                            };

                            dispatch_table.push(quote! {
                                casper_sdk::sys::EntryPoint {
                                    selector: #selector,
                                    fptr: {
                                        #handle_dispatch
                                        #dispatch_func_name::<T>
                                    },
                                    flags: 0, // TODO?
                                }
                            });
                            entry_point_index += 1;

                            let ident = format_ident!(
                                "{trait_name}_{name}",
                                trait_name = trait_name,
                                name = func_name
                            );

                            let input_data_content = if arg_names.is_empty() {
                                quote! {
                                    None
                                }
                            } else {
                                quote! {
                                    Some(borsh::to_vec(&self).expect("Serialization to succeed"))
                                }
                            };
                            let self_ty = if method_attribute.constructor {
                                None
                            } else {
                                Some(quote! {
                                    self,
                                })
                            };
                            extra_code.push(quote! {
                                fn #func_name<'a>(#self_ty #(#arg_names: #arg_types,)*) -> impl casper_sdk::ToCallData<Return<'a> = #call_data_return_lifetime> {
                                    #[derive(BorshSerialize)]
                                    struct #ident {
                                        #(pub #arg_names: #arg_types,)*
                                    }

                                    impl casper_sdk::ToCallData for #ident {
                                        const SELECTOR: casper_sdk::Selector = casper_sdk::Selector::new(#selector);
                                        type Return<'a> = #call_data_return_lifetime;

                                        fn input_data(&self) -> Option<Vec<u8>> {
                                            #input_data_content
                                        }
                                    }


                                    #ident {
                                        #(#arg_names,)*
                                    }
                                }
                            });
                        }
                        syn::TraitItem::Type(_) => todo!("Type"),
                        syn::TraitItem::Macro(_) => todo!("Macro"),
                        syn::TraitItem::Verbatim(_) => todo!("Verbatim"),
                        other => todo!("Other {other:?}"),
                    }
                }

                // let dispatch_struct_name = format_ident!("{trait_name}Dispatch");
                let ref_struct = format_ident!("{trait_name}Ref");
                let ref_struct_trait = format_ident!("{trait_name}Ext");

                let manifest_data_len = dispatch_table.len();

                let extension_struct = quote! {
                    #vis trait #ref_struct_trait: Sized {
                        #(#extra_code)*
                    }


                    #vis struct #ref_struct;

                    impl #ref_struct {

                        #[doc(hidden)]
                        #vis const fn __casper_new_trait_dispatch_table<T: #trait_name + borsh::BorshDeserialize + borsh::BorshSerialize + casper_sdk::Contract + Default>() -> [casper_sdk::sys::EntryPoint; #manifest_data_len] {
                            // This will create set of extern "C" function pointers that will dispatch to a concerete implementation.
                            // Essentially, this constructor provides 'late binding' and crates function pointers for any concrete implementation of given trait.
                            [
                                #(#dispatch_table,)*
                            ]
                        }
                        #[doc(hidden)]
                        #vis fn __casper_populate_definitions(definitions: &mut casper_sdk::abi::Definitions) {
                            #(#populate_definitions)*;
                        }

                        #[doc(hidden)]
                        #vis fn __casper_schema_entry_points() -> Vec<casper_sdk::schema::SchemaEntryPoint> {
                            vec![
                                #(#schema_entry_points,)*
                            ]
                        }
                    }

                    impl #ref_struct_trait for #ref_struct {}

                        impl casper_sdk::schema::CasperSchema for #ref_struct {
                            fn schema() -> casper_sdk::schema::Schema {
                                const VERSION: &str = env!("CARGO_PKG_VERSION");

                                let entry_points = Self::__casper_schema_entry_points();

                                let definitions = {
                                    let mut definitions = casper_sdk::abi::Definitions::default();
                                    Self::__casper_populate_definitions(&mut definitions);
                                    definitions
                                };

                                casper_sdk::schema::Schema {
                                    name: stringify!(#trait_name).into(),
                                    version: Some(VERSION.into()),
                                    type_: casper_sdk::schema::SchemaType::Interface,
                                    definitions,
                                    entry_points,
                                }
                            }
                        }

                        impl casper_sdk::ContractRef for #ref_struct {
                            fn new() -> Self {
                                #ref_struct
                            }
                        }
                };

                return quote! {
                    #item_trait

                    #extension_struct
                }
                .into();
            }

            proc_macro::TokenTree::Ident(ident) if ident.to_string() == "contract" => {
                // if let
                if let Ok(mut entry_points) = syn::parse::<ItemImpl>(item.clone()) {
                    // ident.
                    let mut populate_definitions = Vec::new();

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

                    let mut defs = Vec::new();

                    let mut names = Vec::new();

                    let mut manifest_entry_points_data = Vec::new();
                    let mut manifest_entry_points = Vec::new();
                    let mut manifest_entry_point_enum_variants = Vec::new();
                    let mut manifest_entry_point_enum_match_name = Vec::new();
                    let mut manifest_entry_point_input_data = Vec::new();

                    let mut extra_code = Vec::new();

                    for entry_point in &mut entry_points.items {
                        let method_attribute;
                        let mut flag_value = EntryPointFlags::empty();

                        let func = match entry_point {
                            syn::ImplItem::Const(_) => todo!("Const"),
                            syn::ImplItem::Fn(ref mut func) => {
                                match &func.vis {
                                    syn::Visibility::Public(_) => {}
                                    syn::Visibility::Inherited => {
                                        // As the doc says this "usually means private"
                                        continue;
                                    }
                                    syn::Visibility::Restricted(_restricted) => {}
                                }

                                method_attribute =
                                    MethodAttribute::from_attributes(&func.attrs).unwrap();
                                func.attrs.clear();

                                let name = func.sig.ident.clone();
                                names.push(name.clone());

                                let arg_names_and_types = func
                                    .sig
                                    .inputs
                                    .iter()
                                    .filter_map(|arg| match arg {
                                        syn::FnArg::Receiver(_) => None,
                                        syn::FnArg::Typed(typed) => match typed.pat.as_ref() {
                                            syn::Pat::Ident(ident) => {
                                                Some((&ident.ident, &typed.ty))
                                            }
                                            _ => todo!(),
                                        },
                                    })
                                    .collect::<Vec<_>>();

                                let arg_names: Vec<_> =
                                    arg_names_and_types.iter().map(|(name, _ty)| name).collect();
                                let arg_types: Vec<_> =
                                    arg_names_and_types.iter().map(|(_name, ty)| ty).collect();

                                let arg_count = arg_names.len();

                                // Entry point has &self or &mut self
                                let mut entry_point_requires_state: bool = false;

                                let handle_write_state = match func.sig.inputs.first() {
                                    Some(syn::FnArg::Receiver(receiver))
                                        if receiver.mutability.is_some() =>
                                    {
                                        entry_point_requires_state = true;

                                        if receiver.reference.is_some() {
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
                                    Some(syn::FnArg::Receiver(receiver))
                                        if receiver.mutability.is_none() =>
                                    {
                                        entry_point_requires_state = true;

                                        // &self does not write state
                                        None
                                    }
                                    Some(syn::FnArg::Receiver(receiver))
                                        if receiver.lifetime().is_some() =>
                                    {
                                        panic!("Lifetimes are currently not supported");
                                    }
                                    Some(_) | None => None,
                                };

                                let preamble = if method_attribute.constructor {
                                    let sig = &func.sig;
                                    match func.sig.inputs.first() {
                                        Some(syn::FnArg::Receiver(_receiver)) => {
                                            panic!("Constructor should not take a receiver")
                                        }
                                        _ => {}
                                    }
                                    match &func.sig.output {
                                        syn::ReturnType::Default => {
                                            panic!(
                                            "Constructor should return an instance of the struct"
                                        );
                                        }
                                        syn::ReturnType::Type(_, ty) => match ty.as_ref() {
                                            Type::Never(_) => {
                                                panic!("Constructors should have a return value")
                                            }
                                            ty2 => {
                                                quote! {
                                                    static_assertions::assert_type_eq_all(#struct_name, #ty2);
                                                }
                                            }
                                        },
                                    }
                                } else {
                                    quote! {}
                                };

                                let call_data_return_lifetime = if method_attribute.constructor {
                                    quote! {
                                        #struct_name
                                    }
                                } else {
                                    generate_call_data_return(&func.sig.output)
                                };

                                let handle_ret = match func.sig.output {
                                    syn::ReturnType::Default => {
                                        // Do not call casper_return if there is no return value
                                        None
                                    }
                                    _ => {
                                        // There is a return value so call casper_return.
                                        Some(quote! {
                                            let ret_bytes = borsh::to_vec(&_ret).unwrap();
                                            casper_sdk::host::casper_return(flags, Some(&ret_bytes));
                                        })
                                    }
                                };

                                assert_eq!(arg_names.len(), arg_types.len());

                                let mut entrypoint_params = Vec::new();

                                for (name, ty) in &arg_names_and_types {
                                    entrypoint_params.push(quote! {
                                        {
                                            casper_sdk::sys::Param {
                                                name_ptr: stringify!(#name).as_ptr(),
                                                name_len: stringify!(#name).len(),
                                            }
                                        }
                                    });
                                }

                                let handle_err;
                                let handle_call;

                                if method_attribute.constructor {
                                    manifest_entry_points_data.push(quote! {

                                    #[allow(non_upper_case_globals)]
                                    const #name: (&'static str, [casper_sdk::sys::Param; #arg_count], extern "C" fn() -> ()) = {
                                        extern "C" fn #name() {
                                            let _ret = casper_sdk::host::start(|(#(#arg_names,)*):(#(#arg_types,)*)| {
                                                <#struct_name>::#name(#(#arg_names,)*)
                                            });
                                        }
                                        (stringify!(#name), [#(#entrypoint_params,)*], #name)
                                    };

                                });

                                    // handle_call = quote! {};
                                    handle_err = quote! {};
                                } else {
                                    handle_err = if method_attribute.revert_on_error {
                                        if let syn::ReturnType::Default = func.sig.output {
                                            panic!(
                                            "Cannot revert on error if there is no return value"
                                        );
                                        }

                                        quote! {
                                            let _ret: &Result<_, _> = &_ret;
                                            if _ret.is_err() {
                                                flags |= vm_common::flags::ReturnFlags::REVERT;
                                            }
                                        }
                                    } else {
                                        quote! {}
                                    };
                                }

                                handle_call = if entry_point_requires_state {
                                    quote! {
                                        let mut instance: #struct_name = casper_sdk::host::read_state().unwrap();

                                        let _ret = casper_sdk::host::start_noret(|(#(#arg_names,)*):(#(#arg_types,)*)| {
                                            instance.#name(#(#arg_names,)*)
                                        });
                                    }
                                } else {
                                    quote! {
                                        let _ret = casper_sdk::host::start_noret(|(#(#arg_names,)*):(#(#arg_types,)*)| {
                                            <#struct_name>::#name(#(#arg_names,)*)
                                        });
                                    }
                                };
                                if method_attribute.constructor {
                                    flag_value |= EntryPointFlags::CONSTRUCTOR;
                                }

                                let bits = flag_value.bits();

                                let name_str = name.to_string();

                                let selector = compute_selector(name_str.as_bytes()).get();

                                manifest_entry_points.push(quote! {
                                {

                                    #[allow(non_upper_case_globals)]
                                        extern "C" fn #name() {
                                            let mut flags = vm_common::flags::ReturnFlags::empty();

                                            #handle_call;

                                            #handle_err;

                                            #handle_write_state;

                                            #handle_ret;
                                        }

                                    casper_sdk::sys::EntryPoint {
                                        selector: #selector,
                                        fptr: #name,
                                        flags: #bits,
                                    }
                                }
                            });

                                manifest_entry_point_enum_variants.push(quote! {
                                    #name {
                                        #(#arg_names: #arg_types,)*
                                    }
                                });

                                manifest_entry_point_enum_match_name.push(quote! {
                                    #name
                                });

                                manifest_entry_point_input_data.push(quote! {
                                    Self::#name { #(#arg_names,)* } => {
                                        let into_tuple = (#(#arg_names,)*);
                                        into_tuple.serialize(writer)
                                    }
                                });

                                match entry_points.self_ty.as_ref() {
                                    Type::Path(ref path) => {
                                        let ident = syn::Ident::new(
                                            &format!("{}_{}", path.path.get_ident().unwrap(), name),
                                            Span::call_site(),
                                        );

                                        let input_data_content = if arg_names.is_empty() {
                                            quote! {
                                                None
                                            }
                                        } else {
                                            quote! {
                                                Some(borsh::to_vec(&self).expect("Serialization to succeed"))
                                            }
                                        };

                                        let self_ty = if method_attribute.constructor {
                                            None
                                        } else {
                                            Some(quote! {
                                               &self,
                                            })
                                        };

                                        extra_code.push(quote! {
                                            pub fn #name<'a>(#self_ty #(#arg_names: #arg_types,)*) -> impl casper_sdk::ToCallData<Return<'a> = #call_data_return_lifetime> {
                                                #[derive(BorshSerialize, PartialEq, Debug)]
                                                struct #ident {
                                                    #(#arg_names: #arg_types,)*
                                                }

                                                impl casper_sdk::ToCallData for #ident {
                                                    const SELECTOR: casper_sdk::Selector = casper_sdk::Selector::new(#selector);
                                                    type Return<'a> = #call_data_return_lifetime;

                                                    fn input_data(&self) -> Option<Vec<u8>> {
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

                                func.clone()
                            }
                            syn::ImplItem::Type(_) => todo!(),
                            syn::ImplItem::Macro(_) => todo!(),
                            syn::ImplItem::Verbatim(_) => todo!(),
                            _ => todo!(),
                        };

                        let func_name = &func.sig.ident;

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
                        let bits = flag_value.bits();

                        let selector = compute_selector(func_name.to_string().as_bytes()).get();

                        defs.push(quote! {
                            casper_sdk::schema::SchemaEntryPoint {
                                name: stringify!(#func_name).into(),
                                selector: #selector,
                                arguments: vec![ #(#args,)* ],
                                result: #result,
                                flags: vm_common::flags::EntryPointFlags::from_bits(#bits).unwrap(),
                            }
                        });
                    }

                    // Create a expansion token from the length of `manifest_entry_points_data`
                    // let manifest_entry_points_data_len = manifest_entry_points_data.len();
                    let manifest_entry_points_len = manifest_entry_points.len();

                    let st_name = struct_name.get_ident().unwrap();

                    // let static_entry_points_name = format_ident!("{st_name}_MANIFEST");

                    // Auto generated by a trait
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
                                #[doc(hidden)]
                                fn __casper_schema() -> casper_sdk::schema::Schema {
                                    const VERSION: &str = env!("CARGO_PKG_VERSION");

                                    let mut entry_points = vec![
                                        #(#defs,)*
                                    ];

                                    let definitions = {
                                        let mut definitions = casper_sdk::abi::Definitions::default();
                                        <#struct_name as casper_sdk::abi::CasperABI>::populate_definitions(&mut definitions);
                                        #(#populate_definitions)*;
                                        definitions
                                    };

                                    let state = <#struct_name as casper_sdk::abi::CasperABI>::declaration();
                                    casper_sdk::schema::Schema {
                                        name: stringify!(#struct_name).into(),
                                        type_: casper_sdk::schema::SchemaType::Contract {
                                            state,
                                        },
                                        version: Some(VERSION.into()),
                                        definitions,
                                        entry_points,
                                    }
                                }

                                #[doc(hidden)]
                                const MANIFEST: [casper_sdk::sys::EntryPoint; #manifest_entry_points_len] = [#(#manifest_entry_points,)*];
                            }

                        }),
                    };

                    let ext_struct_name = format_ident!("{st_name}Ref");

                    let res = quote! {
                        #entry_points

                        #handle_manifest

                        impl #ext_struct_name {
                            #(#extra_code)*
                        }

                        impl casper_sdk::ContractRef for #ext_struct_name {
                            fn new() -> Self {
                                #ext_struct_name
                            }
                        }
                    };

                    return res.into();
                } else {
                    let err = syn::Error::new(
                        Span::call_site(),
                        "Entry points can be defined on a trait or an impl block.",
                    );
                    return TokenStream::from(err.to_compile_error());
                }
            }

            proc_macro::TokenTree::Ident(ident) if ident.to_string() == "contract" => {
                todo!()
            }
            proc_macro::TokenTree::Ident(ident) if ident.to_string() == "export" => {
                let func = parse_macro_input!(item as ItemFn);
                let func_name = &func.sig.ident;

                // let mut arg_slices = Vec::new();
                // let mut arg_casts = Vec::new();
                let mut arg_names = Vec::new();
                let mut arg_types = Vec::new();
                // let mut tuple_args = Vec::new::new();

                for input in &func.sig.inputs {
                    let (name, ty) = match input {
                        syn::FnArg::Receiver(receiver) => {
                            todo!("{receiver:?}")
                        }
                        syn::FnArg::Typed(typed) => match typed.pat.as_ref() {
                            syn::Pat::Ident(ident) => (&ident.ident, &typed.ty),
                            _ => todo!(),
                        },
                    };
                    arg_names.push(name.clone());
                    arg_types.push(ty.clone());
                }

                // let arg_tokens =
                // let

                // let inputs = &func.sig.inputs;
                // Ident::
                let mod_name = format_ident!("__casper__export_{func_name}");
                let ctor_name = format_ident!("{func_name}_ctor");
                // let export_name = format_ident!("NAME_{func_name}");
                // let export_args = format_ident!("ARGS_{func_name}");

                let token = quote! {
                    pub(crate) mod #mod_name {
                        use super::*;
                        use borsh::BorshDeserialize;

                        // #[cfg(not(target_arch = "wasm32")]
                        // use ctor::ctor;

                        #func

                    }

                    #[cfg(target_arch = "wasm32")]
                    #[no_mangle]
                    pub extern "C" fn #func_name() {
                        // Set panic hook (assumes std is enabled etc.)
                        casper_sdk::set_panic_hook();

                        // TODO: If signature has no args we don't need to deserialize anything
                        use borsh::BorshDeserialize;

                        // ("foo", 1234) -> input

                        let input = casper_sdk::host::casper_copy_input();

                        let ( #(#arg_names,)* ) = BorshDeserialize::try_from_slice(&input).unwrap();

                        #mod_name::#func_name(#(#arg_names,)*);
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    fn #func_name(input: &[u8]) {
                        use borsh::BorshDeserialize;
                        let ( #(#arg_names,)* ) = BorshDeserialize::try_from_slice(input).unwrap();
                        #mod_name::#func_name(#(#arg_names,)*);
                    }

                    #[cfg(not(target_arch = "wasm32"))]
                    #[ctor::ctor]
                    fn #ctor_name() {
                        let export = casper_sdk::schema::schema_helper::Export {
                            name: stringify!(#func_name),
                            fptr: #func_name,
                        };

                        casper_sdk::schema::schema_helper::register_export(export);
                    }
                    // #[cfg(not(target_arch = "wasm32"))]
                    // pub use #mod_name::{#func_name};

                    // #[cfg(not(target_arch = "wasm32"))]

                    // pub fn #func_name(input: Vec<u8>) {
                    //     use borsh::BorshDeserialize;
                    //     let ( #(#arg_names,)* ) = BorshDeserialize::try_from_slice(&input).unwrap();
                    //     #mod_name::#func_name(#(arg_names,)*);
                    // }
                };

                return token.into();
            }
            other => todo!("other attribute {other:?}"),
        }
    }
    todo!()
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

const PRIMITIVE_TYPES: &[&str] = &[
    "u8", "u16", "u32", "u64", "u128", "i8", "i16", "i32", "i64", "i128", "bool", "String", "f32",
    "f64",
];

#[proc_macro_derive(CasperSchema, attributes(casper))]
pub fn derive_casper_schema(input: TokenStream) -> TokenStream {
    let contract = parse_macro_input!(input as DeriveInput);

    let contract_attributes = ContractAttributes::from_attributes(&contract.attrs).unwrap();

    let data_struct = match &contract.data {
        Data::Struct(s) => s,
        Data::Enum(_) => todo!("Enum"),
        Data::Union(_) => todo!("Union"),
    };

    let name = &contract.ident;

    let mut extra_code = Vec::new();
    if let Some(traits) = contract_attributes.impl_traits {
        for path in traits.iter() {
            let ext_struct = format_ident!("{}Ref", path.require_ident().unwrap());
            extra_code.push(quote! {
                {
                    let entry_points = <#ext_struct>::__casper_schema_entry_points();
                    schema.entry_points.extend(entry_points);
                    <#ext_struct>::__casper_populate_definitions(&mut schema.definitions);
                }
            });
        }
    }

    quote! {
        impl casper_sdk::schema::CasperSchema for #name {
            fn schema() -> casper_sdk::schema::Schema {
                let mut schema = Self::__casper_schema();

                #(#extra_code)*;

                schema
                // schema.entry_points.ext
            }
        }
    }
    .into()
}

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

    let selector = compute_selector(bytes).get();

    TokenStream::from(quote! {
        casper_sdk::Selector::new(#selector)
    })
    .into()
}

pub(crate) fn compute_selector(bytes: &[u8]) -> Selector {
    let hash_bytes = {
        let mut context = blake2_rfc::blake2b::Blake2b::new(32);
        context.update(&bytes);
        context.finalize()
    };

    let selector_bytes: [u8; 4] = (&hash_bytes.as_bytes()[0..4]).try_into().unwrap();

    // Using be constructor from first 4 bytes in big endian order should basically copy first 4
    // bytes in order into the integer.
    let selector = u32::from_be_bytes(selector_bytes);

    Selector::new(selector)
}

#[proc_macro]
pub fn test(item: TokenStream) -> TokenStream {
    // #[casper::test] fn foo() { }
    let input = parse_macro_input!(item as ItemFn);
    TokenStream::from(quote! {
        #[test]
        #input
    })
    .into()
}
