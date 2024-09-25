pub mod support;

use casper_sdk::{
    abi::{Declaration, Definition, Primitive},
    casper_executor_wasm_common::flags::EntryPointFlags,
    schema::{Schema, SchemaType},
};
use codegen::{Field, Scope, Type};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    iter,
    str::FromStr,
};

const DEFAULT_DERIVED_TRAITS: &[&str] = &[
    "Clone",
    "Debug",
    "PartialEq",
    "Eq",
    "PartialOrd",
    "Ord",
    "Hash",
    "BorshSerialize",
    "BorshDeserialize",
];

/// Replaces characters that are not valid in Rust identifiers with underscores.
fn slugify_type(input: &str) -> String {
    let mut output = String::with_capacity(input.len());

    for c in input.chars() {
        if c.is_ascii_alphanumeric() {
            output.push(c);
        } else {
            output.push('_');
        }
    }

    output
}

#[derive(Debug, Deserialize, Serialize)]
enum Specialized {
    Result { ok: Declaration, err: Declaration },
    Option { some: Declaration },
}

#[derive(Deserialize, Serialize)]
pub struct Codegen {
    schema: Schema,
    type_mapping: BTreeMap<Declaration, String>,
    specialized_types: BTreeMap<Declaration, Specialized>,
}

impl FromStr for Codegen {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let schema: Schema = serde_json::from_str(s)?;
        Ok(Self::new(schema))
    }
}

impl Codegen {
    pub fn new(schema: Schema) -> Self {
        Self {
            schema,
            type_mapping: Default::default(),
            specialized_types: Default::default(),
        }
    }

    pub fn from_file(path: &str) -> Result<Self, std::io::Error> {
        let file = std::fs::File::open(path)?;
        let schema: Schema = serde_json::from_reader(file)?;
        Ok(Self::new(schema))
    }

    pub fn gen(&mut self) -> String {
        let mut scope = Scope::new();

        scope.import("borsh", "self");
        scope.import("borsh", "BorshSerialize");
        scope.import("borsh", "BorshDeserialize");
        scope.import("casper_sdk_codegen::support", "IntoResult");
        scope.import("casper_sdk_codegen::support", "IntoOption");
        scope.import("casper_sdk", "Selector");
        scope.import("casper_sdk", "ToCallData");

        let _head = self
            .schema
            .definitions
            .first()
            .expect("No definitions found.");

        match &self.schema.type_ {
            SchemaType::Contract { state } => {
                if !self.schema.definitions.has_definition(state) {
                    panic!(
                        "Missing state definition. Expected to find a definition for {}.",
                        &state
                    )
                };
            }
            SchemaType::Interface => {}
        }

        // Initialize a queue with the first definition
        let mut queue = VecDeque::new();

        // Create a set to keep track of processed definitions
        let mut processed = std::collections::HashSet::new();

        let mut graph: IndexMap<_, VecDeque<_>> = IndexMap::new();

        for (def_index, (next_decl, next_def)) in self.schema.definitions.iter().enumerate() {
            println!(
                "{def_index}. decl={decl}",
                def_index = def_index,
                decl = next_decl
            );

            queue.push_back(next_decl);

            while let Some(decl) = queue.pop_front() {
                if processed.contains(decl) {
                    continue;
                }

                processed.insert(decl);
                graph.entry(next_decl).or_default().push_back(decl);
                // graph.find

                match Primitive::from_str(decl) {
                    Ok(primitive) => {
                        println!("Processing primitive type {primitive:?}");
                        continue;
                    }
                    Err(_) => {
                        // Not a primitive type
                    }
                };

                let def = self
                    .schema
                    .definitions
                    .get(decl)
                    .unwrap_or_else(|| panic!("Missing definition for {}", decl));

                // graph.entry(next_decl).or_default().push(decl);
                // println!("Processing type {decl}");

                // Enqueue all unprocessed definitions that depend on the current definition
                match def {
                    Definition::Primitive(_primitive) => {
                        continue;
                    }
                    Definition::Mapping { key, value } => {
                        if !processed.contains(key) {
                            queue.push_front(key);
                            continue;
                        }

                        if !processed.contains(value) {
                            queue.push_front(value);
                            continue;
                        }
                    }
                    Definition::Sequence { decl } => {
                        queue.push_front(decl);
                    }
                    Definition::FixedSequence { length: _, decl } => {
                        if !processed.contains(decl) {
                            queue.push_front(decl);
                            continue;
                        }
                    }
                    Definition::Tuple { items } => {
                        for item in items {
                            if !processed.contains(item) {
                                queue.push_front(item);
                                continue;
                            }
                        }

                        // queue.push_front(decl);
                    }
                    Definition::Enum { items } => {
                        for item in items {
                            if !processed.contains(&item.decl) {
                                queue.push_front(&item.decl);
                                continue;
                            }
                        }
                    }
                    Definition::Struct { items } => {
                        for item in items {
                            if !processed.contains(&item.decl) {
                                queue.push_front(&item.decl);
                                continue;
                            }
                        }
                    }
                }
            }

            match next_def {
                Definition::Primitive(_) => {}
                Definition::Mapping { key, value } => {
                    assert!(processed.contains(key));
                    assert!(processed.contains(value));
                }
                Definition::Sequence { decl } => {
                    assert!(processed.contains(decl));
                }
                Definition::FixedSequence { length: _, decl } => {
                    assert!(processed.contains(decl));
                }
                Definition::Tuple { items } => {
                    for item in items {
                        assert!(processed.contains(&item));
                    }
                }
                Definition::Enum { items } => {
                    for item in items {
                        assert!(processed.contains(&item.decl));
                    }
                }
                Definition::Struct { items } => {
                    for item in items {
                        assert!(processed.contains(&item.decl));
                    }
                }
            }
        }
        dbg!(&graph);

        let mut counter = iter::successors(Some(0usize), |prev| prev.checked_add(1));

        for (_decl, deps) in graph {
            for decl in deps.into_iter().rev() {
                // println!("generate {decl}");

                let def = self
                    .schema
                    .definitions
                    .get(decl)
                    .cloned()
                    .or_else(|| Primitive::from_str(decl).ok().map(Definition::Primitive))
                    .unwrap_or_else(|| panic!("Missing definition for {}", decl));

                match def {
                    Definition::Primitive(primitive) => {
                        let (from, to) = match primitive {
                            Primitive::Char => ("Char", "char"),
                            Primitive::U8 => ("U8", "u8"),
                            Primitive::I8 => ("I8", "i8"),
                            Primitive::U16 => ("U16", "u16"),
                            Primitive::I16 => ("I16", "i16"),
                            Primitive::U32 => ("U32", "u32"),
                            Primitive::I32 => ("I32", "i32"),
                            Primitive::U64 => ("U64", "u64"),
                            Primitive::I64 => ("I64", "i64"),
                            Primitive::U128 => ("U128", "u128"),
                            Primitive::I128 => ("I128", "i128"),
                            Primitive::Bool => ("Bool", "bool"),
                            Primitive::F32 => ("F32", "f32"),
                            Primitive::F64 => ("F64", "f64"),
                        };

                        scope.new_type_alias(from, to).vis("pub");
                        self.type_mapping.insert(decl.to_string(), from.to_string());
                    }
                    Definition::Mapping { key: _, value: _ } => {
                        // println!("Processing mapping type {key:?} -> {value:?}");
                        todo!()
                    }
                    Definition::Sequence { decl: seq_decl } => {
                        println!("Processing sequence type {decl:?}");
                        if decl.as_str() == "String"
                            && Primitive::from_str(&seq_decl) == Ok(Primitive::Char)
                        {
                            self.type_mapping
                                .insert("String".to_owned(), "String".to_owned());
                        } else {
                            let mapped_type = self
                                .type_mapping
                                .get(&seq_decl)
                                .unwrap_or_else(|| panic!("Missing type mapping for {}", seq_decl));
                            let type_name =
                                format!("Sequence{}_{seq_decl}", counter.next().unwrap());
                            scope.new_type_alias(&type_name, format!("Vec<{}>", mapped_type));
                            self.type_mapping.insert(decl.to_string(), type_name);
                        }
                    }
                    Definition::FixedSequence {
                        length,
                        decl: fixed_seq_decl,
                    } => {
                        let mapped_type =
                            self.type_mapping.get(&fixed_seq_decl).unwrap_or_else(|| {
                                panic!("Missing type mapping for {}", fixed_seq_decl)
                            });

                        let type_name = format!(
                            "FixedSequence{}_{length}_{fixed_seq_decl}",
                            counter.next().unwrap()
                        );
                        scope.new_type_alias(&type_name, format!("[{}; {}]", mapped_type, length));
                        self.type_mapping.insert(decl.to_string(), type_name);
                    }
                    Definition::Tuple { items } => {
                        if decl.as_str() == "()" && items.is_empty() {
                            self.type_mapping.insert("()".to_owned(), "()".to_owned());
                            continue;
                        }

                        println!("Processing tuple type {items:?}");
                        let struct_name = slugify_type(decl);

                        let r#struct = scope
                            .new_struct(&struct_name)
                            .doc(&format!("Declared as {decl}"));

                        for trait_name in DEFAULT_DERIVED_TRAITS {
                            r#struct.derive(trait_name);
                        }

                        if items.is_empty() {
                            r#struct.tuple_field(Type::new("()"));
                        } else {
                            for item in items {
                                let mapped_type = self
                                    .type_mapping
                                    .get(&item)
                                    .unwrap_or_else(|| panic!("Missing type mapping for {}", item));
                                r#struct.tuple_field(mapped_type);
                            }
                        }

                        self.type_mapping.insert(decl.to_string(), struct_name);
                    }
                    Definition::Enum { items } => {
                        println!("Processing enum type {decl} {items:?}");

                        let mut items: Vec<&casper_sdk::abi::EnumVariant> = items.iter().collect();

                        let mut specialized = None;

                        if decl.starts_with("Result")
                            && items.len() == 2
                            && items[0].name == "Ok"
                            && items[1].name == "Err"
                        {
                            specialized = Some(Specialized::Result {
                                ok: items[0].decl.clone(),
                                err: items[1].decl.clone(),
                            });

                            // NOTE: Because we're not doing the standard library Result, and also
                            // to simplify things we're using default impl of
                            // BorshSerialize/BorshDeserialize, we have to flip the order of enums.
                            // The standard library defines Result as Ok, Err, but the borsh impl
                            // serializes Err as 0, and Ok as 1. So, by flipping the order we can
                            // enforce byte for byte compatibility between our "custom" Result and a
                            // real Result.
                            items.reverse();
                        }

                        if decl.starts_with("Option")
                            && items.len() == 2
                            && items[0].name == "None"
                            && items[1].name == "Some"
                        {
                            specialized = Some(Specialized::Option {
                                some: items[1].decl.clone(),
                            });

                            items.reverse();
                        }

                        let enum_name = slugify_type(decl);

                        let r#enum = scope
                            .new_enum(&enum_name)
                            .vis("pub")
                            .doc(&format!("Declared as {decl}"));

                        for trait_name in DEFAULT_DERIVED_TRAITS {
                            r#enum.derive(trait_name);
                        }

                        for item in &items {
                            let variant = r#enum.new_variant(&item.name);

                            let def = self.type_mapping.get(&item.decl).unwrap_or_else(|| {
                                panic!("Missing type mapping for {}", item.decl)
                            });

                            variant.tuple(def);
                        }

                        self.type_mapping
                            .insert(decl.to_string(), enum_name.to_owned());

                        match specialized {
                            Some(Specialized::Result { ok, err }) => {
                                let ok_type = self
                                    .type_mapping
                                    .get(&ok)
                                    .unwrap_or_else(|| panic!("Missing type mapping for {}", ok));
                                let err_type = self
                                    .type_mapping
                                    .get(&err)
                                    .unwrap_or_else(|| panic!("Missing type mapping for {}", err));

                                let impl_block = scope
                                    .new_impl(&enum_name)
                                    .impl_trait(format!("IntoResult<{ok_type}, {err_type}>"));

                                let func = impl_block.new_fn("into_result").arg_self().ret(
                                    Type::new(format!(
                                        "Result<{ok_type}, {err_type}>",
                                        ok_type = ok_type,
                                        err_type = err_type
                                    )),
                                );
                                func.line("match self {")
                                    .line(format!("{enum_name}::Ok(ok) => Ok(ok),"))
                                    .line(format!("{enum_name}::Err(err) => Err(err),"))
                                    .line("}");
                            }
                            Some(Specialized::Option { some }) => {
                                let some_type = self.type_mapping.get(&some).unwrap_or_else(|| {
                                    panic!("Missing type mapping for {}", &some)
                                });

                                let impl_block = scope
                                    .new_impl(&enum_name)
                                    .impl_trait(format!("IntoOption<{some_type}>"));

                                let func = impl_block
                                    .new_fn("into_option")
                                    .arg_self()
                                    .ret(Type::new(format!("Option<{some_type}>",)));
                                func.line("match self {")
                                    .line(format!("{enum_name}::None => None,"))
                                    .line(format!("{enum_name}::Some(some) => Some(some),"))
                                    .line("}");
                            }
                            None => {}
                        }
                    }
                    Definition::Struct { items } => {
                        println!("Processing struct type {items:?}");

                        let type_name = slugify_type(decl);

                        let r#struct = scope.new_struct(&type_name);

                        for trait_name in DEFAULT_DERIVED_TRAITS {
                            r#struct.derive(trait_name);
                        }

                        for item in items {
                            let mapped_type =
                                self.type_mapping.get(&item.decl).unwrap_or_else(|| {
                                    panic!("Missing type mapping for {}", item.decl)
                                });
                            let field = Field::new(&item.name, Type::new(mapped_type))
                                .doc(&format!("Declared as {}", item.decl))
                                .to_owned();

                            r#struct.push_field(field);
                        }
                        self.type_mapping.insert(decl.to_string(), type_name);
                    }
                }
            }
        }

        let struct_name = format!("{}Client", self.schema.name);
        let client = scope.new_struct(&struct_name).vis("pub");

        for trait_name in DEFAULT_DERIVED_TRAITS {
            client.derive(trait_name);
        }

        let mut field = Field::new("address", Type::new("[u8; 32]"));
        field.vis("pub");

        client.push_field(field);

        let client_impl = scope.new_impl(&struct_name);

        for entry_point in &self.schema.entry_points {
            let func = client_impl.new_fn(&entry_point.name);
            func.vis("pub");

            let result_type = self
                .type_mapping
                .get(&entry_point.result)
                .unwrap_or_else(|| panic!("Missing type mapping for {}", entry_point.result));

            if entry_point.flags.contains(EntryPointFlags::CONSTRUCTOR) {
                func.ret(Type::new(format!(
                    "Result<{}, casper_sdk::types::CallError>",
                    &struct_name
                )))
                .generic("C")
                .bound("C", "casper_sdk::Contract");
            } else {
                func.ret(Type::new(format!(
                    "Result<casper_sdk::host::CallResult<{result_type}>, casper_sdk::types::CallError>"
                )));
                func.arg_ref_self();
            }

            for arg in &entry_point.arguments {
                let mapped_type = self
                    .type_mapping
                    .get(&arg.decl)
                    .unwrap_or_else(|| panic!("Missing type mapping for {}", arg.decl));
                let arg_ty = Type::new(mapped_type);
                func.arg(&arg.name, arg_ty);
            }

            func.line(format!(
                r#"const SELECTOR: Selector = Selector::new({});"#,
                entry_point
                    .selector
                    .expect("TODO: Handle fallback entrypoint"),
            ));

            func.line("let value = 0; // TODO: Transferring values");

            let input_struct_name =
                format!("{}_{}", slugify_type(&self.schema.name), &entry_point.name);

            if entry_point.arguments.is_empty() {
                func.line(format!(r#"let call_data = {input_struct_name};"#));
            } else {
                func.line(format!(r#"let call_data = {input_struct_name} {{ "#));
                for arg in &entry_point.arguments {
                    func.line(format!("{},", arg.name));
                }
                func.line("};");
            }

            if entry_point.flags.contains(EntryPointFlags::CONSTRUCTOR) {
                // if !entry_point.arguments.is_empty() {
                //     func.line(r#"let create_result = C::create(SELECTOR, Some(&input_data))?;"#);
                // } else {
                func.line(r#"let create_result = C::create(call_data)?;"#);
                // }

                func.line(format!(
                    r#"let result = {struct_name} {{ address: create_result.contract_address }};"#,
                    struct_name = &struct_name
                ));
                func.line("Ok(result)");
                continue;
            } else {
                func.line(r#"casper_sdk::host::call(&self.address, value, call_data)"#);
            }
        }

        for entry_point in &self.schema.entry_points {
            // Generate arg structure similar to what casper-macros is doing
            let struct_name = format!("{}_{}", &self.schema.name, &entry_point.name);
            let input_struct = scope.new_struct(&struct_name);

            for trait_name in DEFAULT_DERIVED_TRAITS {
                input_struct.derive(trait_name);
            }

            for argument in &entry_point.arguments {
                let mapped_type = self.type_mapping.get(&argument.decl).unwrap_or_else(|| {
                    panic!(
                        "Missing type mapping for {} when generating input arg {}",
                        argument.decl, &struct_name
                    )
                });
                input_struct.push_field(Field::new(&argument.name, Type::new(mapped_type)));
            }

            let impl_block = scope.new_impl(&struct_name).impl_trait("ToCallData");

            impl_block.associate_const(
                "SELECTOR",
                "Selector",
                format!(
                    "Selector::new({})",
                    entry_point.selector.expect("Handle fallback")
                ),
                String::new(),
            );

            let input_data_func = impl_block
                .new_fn("input_data")
                .arg_ref_self()
                .ret(Type::new("Option<Vec<u8>>"));

            if entry_point.arguments.is_empty() {
                input_data_func.line(r#"None"#);
            } else {
                input_data_func
                        .line(r#"let input_data = borsh::to_vec(&self).expect("Serialization to succeed");"#)
                        .line(r#"Some(input_data)"#);
            }
        }

        scope.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_slugify_complex_type() {
        let input = "Option<Result<(), vm2_cep18::error::Cep18Error>>";
        let expected = "Option_Result_____vm2_cep18__error__Cep18Error__";

        assert_eq!(slugify_type(input), expected);
    }
}
