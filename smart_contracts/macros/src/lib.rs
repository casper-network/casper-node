extern crate proc_macro;

use proc_macro::{Literal, TokenStream};
use quote::{format_ident, quote};
use syn::{parse_macro_input, token::Struct, Data, DeriveInput, ItemFn, ItemImpl, Type};

#[proc_macro_derive(Contract)]
pub fn derive_casper_contract(input: TokenStream) -> TokenStream {
    // Parse the input tokens into a syntax tree
    // let input = parse_macro_input!(input as DeriveInput);
    let contract = parse_macro_input!(input as DeriveInput);
    // todo!("{contract:?}")
    let name = &contract.ident;
    let _vis = &contract.vis;

    let data_struct = match &contract.data {
        Data::Struct(s) => s,
        Data::Enum(_) => todo!("Enum"),
        Data::Union(_) => todo!("Union"),
    };

    let mut fields = Vec::new();
    let mut fields_for_schema = Vec::new();

    // let fields = data_struct.fields;
    let mut fields_for_new = Vec::new();
    for field in &data_struct.fields {
        let name = &field.ident;
        let ty = &field.ty;
        // fields.push(field.clone());
        fields.push(quote! {
            #[allow(dead_code)]
            #name: casper_sdk::Value<#ty>
        });

        fields_for_schema.push(quote! {
            casper_sdk::SchemaData {
                name: stringify!(#name),
                ty: {
                    use casper_sdk::CLTyped;
                    <#ty>::cl_type()
                },
            }
        });

        fields_for_new.push(quote! {
            #name: casper_sdk::Value::new(stringify!(#name), 0)
        })
    }

    let static_metadata = format_ident!("{name}EntryPoint");

    let f = quote! {

        impl casper_sdk::Contract for #name {
            type EntryPoint = ();

            fn new() -> Self {
                Self {
                    #(#fields_for_new,)*
                }
            }
            fn name() -> &'static str {
                stringify!(#name)
            }

            fn schema() -> casper_sdk::Schema {
                Self::__casper_schema()
            }

            fn create() -> Result<casper_sdk::host::CreateResult, casper_sdk::host::Error> {
                Self::__casper_create()
            }
        }

        impl #name {
            #[doc(hidden)]
            fn __casper_data() -> Vec<casper_sdk::SchemaData> {
                vec! [
                    #(#fields_for_schema,)*
                ]
            }
        }
    };
    f.into()
}

#[proc_macro_attribute]
pub fn casper(attrs: TokenStream, item: TokenStream) -> TokenStream {
    // #[casper(foo)]
    // eprintln!("{attrs:?}");

    let mut attrs_iter = attrs.into_iter().peekable();

    for attr in &mut attrs_iter {
        let item = item.clone();
        match attr {
            proc_macro::TokenTree::Ident(ident) if ident.to_string() == "entry_points" => {
                let entry_points = parse_macro_input!(item as ItemImpl);

                let struct_name = match entry_points.self_ty.as_ref() {
                    Type::Path(ref path) => &path.path,

                    other => todo!("{other:?}"),
                };
                // todo!("{:?}", &entry_points);
                // let name = entry_points

                // }

                let mut defs = Vec::new();

                let mut names = Vec::new();

                let mut manifest_entry_points_data = Vec::new();
                let mut manifest_entry_points = Vec::new();
                let mut manifest_entry_point_enum_variants = Vec::new();
                let mut manifest_entry_point_enum_match_name = Vec::new();
                let mut manifest_entry_point_input_data = Vec::new();

                for entry_point in &entry_points.items {
                    let func = match entry_point {
                        syn::ImplItem::Const(_) => todo!(),
                        syn::ImplItem::Fn(func) => {
                            let name = &func.sig.ident;
                            names.push(name);

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

                            let arg_count = arg_names.len();
                            assert_eq!(arg_names.len(), arg_types.len());

                            let mut entrypoint_params = Vec::new();

                            for (name, ty) in &arg_names_and_types {
                                entrypoint_params.push(quote! {
                                    {
                                        casper_sdk::host::Param {
                                            name_ptr: stringify!(#name).as_ptr(),
                                            name_len: stringify!(#name).len(),
                                            ty: <#ty as casper_sdk::CLTyped>::TYPE_ID,
                                        }
                                    }
                                });
                            }

                            // let wrapper_function_name = format_ident!("")

                            manifest_entry_points_data.push(quote! {

                                const #name: (&'static str, [casper_sdk::host::Param; #arg_count], extern "C" fn() -> ()) = {
                                    extern "C" fn #name() {
                                        casper_sdk::host::start(|(#(#arg_names,)*):(#(#arg_types,)*)| {
                                            let mut contract_instance = <#struct_name as casper_sdk::Contract>::new();
                                            let _result = contract_instance.#name(#(#arg_names,)*);
                                        })
                                    }
                                    (stringify!(#name), [#(#entrypoint_params,)*], #name)
                                };

                            });

                            manifest_entry_points.push(quote! {
                                {
                                    casper_sdk::host::EntryPoint {
                                        name_ptr: #name.0.as_ptr(),
                                        name_len: #name.0.len(),
                                        params_ptr: #name.1.as_ptr(),
                                        params_size: #name.1.len(),
                                        fptr: #name.2,
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

                            func
                        }
                        syn::ImplItem::Type(_) => todo!(),
                        syn::ImplItem::Macro(_) => todo!(),
                        syn::ImplItem::Verbatim(_) => todo!(),
                        _ => todo!(),
                    };

                    let func_name = &func.sig.ident;

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
                        args.push(quote! {
                            casper_sdk::SchemaArgument {
                                name: stringify!(#name),
                                ty: {
                                    use casper_sdk::CLTyped;
                                    <#ty>::cl_type()
                                },
                            }
                        });
                    }

                    // let mut args = Vec::new();
                    // for arg in &entry_point

                    defs.push(quote! {
                        casper_sdk::SchemaEntryPoint {
                            name: stringify!(#func_name),
                            arguments: vec![ #(#args,)* ]
                        }
                    });
                }

                // Create a expansion token from the length of `manifest_entry_points_data`
                let manifest_entry_points_data_len = manifest_entry_points_data.len();

                // let static_schema_ident = format_ident!("__casper_schema_{struct_name}");

                let st_name = struct_name.get_ident().unwrap();
                let static_metadata = format_ident!("{st_name}EntryPoint");

                let res = quote! {
                    #entry_points

                    #[doc(hidden)]
                    #[derive(Debug)]
                    pub enum #static_metadata {
                        #(#manifest_entry_point_enum_variants,)*
                    }

                    impl #static_metadata {
                        #[doc(hidden)]
                        pub const fn entry_point(&self) -> &'static str {
                            match self {
                                #(Self::#manifest_entry_point_enum_match_name { .. } => stringify!(#manifest_entry_point_enum_match_name),)*
                            }
                        }
                        #[doc(hidden)]
                        pub fn input_data(&self) -> borsh::io::Result<Vec<u8>> {
                            borsh::to_vec(self)
                        }
                    }

                    impl borsh::BorshSerialize for #static_metadata {
                        fn serialize<W: borsh::io::Write>(&self, writer: &mut W) -> borsh::io::Result<()> {
                            match self {
                                #(#manifest_entry_point_input_data)*
                            }
                            // Ok(())
                        }
                    }

                    impl #struct_name {

                        #[doc(hidden)]
                        fn __casper_schema() -> casper_sdk::Schema {
                            let entry_points = vec![
                                #(#defs,)*
                                // EntryPonit
                            ];
                            let data = Self::__casper_data();
                            casper_sdk::Schema {
                                name: stringify!(#struct_name),
                                data,
                                entry_points,
                            }
                        }

                        #[inline(always)]
                        #[doc(hidden)]
                        fn __casper_create() -> Result<casper_sdk::host::CreateResult, casper_sdk::host::Error> {
                            #(#manifest_entry_points_data)*;
                            const ENTRY_POINTS: [casper_sdk::host::EntryPoint; #manifest_entry_points_data_len] = [#(#manifest_entry_points,)*];

                            const MANIFEST: casper_sdk::host::Manifest = casper_sdk::host::Manifest {
                                entry_points: ENTRY_POINTS.as_ptr(),
                                entry_points_size: ENTRY_POINTS.len(),
                            };

                            casper_sdk::host::create(None, &MANIFEST)
                        }
                    }
                };

                return res.into();
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
                    // let ty = &input.
                    arg_names.push(name.clone());
                    arg_types.push(ty.clone());

                    // let sig = &input.sig.inputs;
                    // // let name = match typed.pat.as_ref() {
                    // //     syn::Pat::Ident(ident) => &ident.ident,
                    // //     _ => todo!(),
                    // // };

                    // // let name = input.n
                    // let arg = quote! {
                    //     unsafe { core::ptr::NonNull::new_unchecked(#name).as_ref() }.as_slice()
                    // };

                    // arg_casts.push(arg);
                    // let arg_slice = quote! {
                    //     #name: *mut casper_sdk::host::Slice
                    // };
                    // arg_slices.push(arg_slice);

                    // arg_calls.push(quote! {
                    //     name
                    // })
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

                        let input = casper_sdk::host::copy_input();
                        // let args = #mod_name::Arguments::try_from_slice(&input).unwrap();
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
                        let export = casper_sdk::schema_helper::Export {
                            name: stringify!(#func_name),
                            fptr: #func_name,
                        };

                        casper_sdk::schema_helper::register_export(export);
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
            _ => todo!(),
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
//     // ItemFn
//     //  syn::parse(function).expect("should be function");
//     // let input2 = item.clone();
//     // let DeriveInput { ident, data, .. } = parse_macro_input!(input2);

//     // println!("attr: \"{}\"", attr.to_string());
//     // println!("item: \"{}\"", item.to_string());

//     // if let Data::
//     // println!("{}", data);
//     // if let
//     // item
// }
