use casper_sdk::{
    abi::{Declaration, Definition, Primitive},
    schema::Schema,
};
use codegen::{Field, Scope, Type};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    iter,
    str::FromStr,
};

#[derive(Deserialize, Serialize)]
pub struct Codegen {
    schema: Schema,
    type_mapping: BTreeMap<Declaration, String>,
}

impl Codegen {
    pub fn new(schema: Schema) -> Self {
        Self {
            schema,
            type_mapping: BTreeMap::new(),
        }
    }

    pub fn from_file(path: &str) -> Result<Self, std::io::Error> {
        let file = std::fs::File::open(path)?;
        let schema: Schema = serde_json::from_reader(file)?;
        Ok(Self::new(schema))
    }

    pub fn gen(&mut self) -> String {
        let mut scope = Scope::new();

        let head = self
            .schema
            .definitions
            .first()
            .expect("No definitions found.");

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
                    Definition::Primitive(primitive) => {
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
                    Definition::FixedSequence { length, decl } => {
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
                    assert!(processed.contains(&decl));
                }
                Definition::FixedSequence { length, decl } => {
                    assert!(processed.contains(&decl));
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
            for decl in deps.iter().rev() {
                // println!("generate {decl}");

                let def = self
                    .schema
                    .definitions
                    .get(decl)
                    .cloned()
                    .or_else(|| {
                        Primitive::from_str(decl)
                            .ok()
                            .map(|primitive| Definition::Primitive(primitive))
                    })
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

                        scope.new_type_alias(from, to);
                        self.type_mapping.insert(decl.to_string(), from.to_string());
                    }
                    Definition::Mapping { key, value } => {
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
                        // println!("Processing fixed sequence type {length:?} * {decl:?}
                        // {mapped_type}"); todo!()
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

                        // println!("Processing tuple type {items:?}");
                        let struct_name = format!("Tuple{}", counter.next().unwrap());

                        let r#struct = scope
                            .new_struct(&struct_name)
                            .doc(&format!("Declared as {decl}"));

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
                        // let tuple = scope.new_type_alias(&alias,
                    }
                    Definition::Enum { items } => {
                        println!("Processing enum type {decl} {items:?}");

                        // let path = decl.split("::");
                        // let enum_name = path.last().expect("Name");
                        let enum_name = format!("Enum{}", counter.next().unwrap());

                        // let type_name = format!("{}", enum_name);
                        let r#enum = scope
                            .new_enum(&enum_name)
                            .doc(&format!("Declared as {decl}"));

                        for item in &items {
                            let variant = r#enum.new_variant(&item.name);

                            let def = self.type_mapping.get(&item.decl).unwrap_or_else(|| {
                                panic!("Missing type mapping for {}", item.decl)
                            });

                            variant.tuple(def);
                            // Where discriminant?
                        }

                        self.type_mapping
                            .insert(decl.to_string(), enum_name.to_owned());

                        // todo!()
                        // .push_variant()
                        // .push_variant("Baz", "u8");
                    }
                    Definition::Struct { items } => {
                        // println!("Processing struct type {items:?}");
                        let type_name = format!("Struct{}", counter.next().unwrap());
                        let r#struct = scope.new_struct(&type_name);
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

        // for (def_index, (decl, def)) in defs.into_iter().enumerate()     {
        //     match def {
        //         Definition::Primitive(primitive) => {
        //             let (from, to) = match primitive {
        //                 Primitive::Char => ("Char", "char"),
        //                 Primitive::U8 => ("U8", "u8"),
        //                 Primitive::I8 => ("I8", "i8"),
        //                 Primitive::U16 => ("U16", "u16"),
        //                 Primitive::I16 => ("I16", "i16"),
        //                 Primitive::U32 => ("U32", "u32"),
        //                 Primitive::I32 => ("I32", "i32"),
        //                 Primitive::U64 => ("U64", "u64"),
        //                 Primitive::I64 => ("I64", "i64"),
        //                 Primitive::U128 => ("U128", "u128"),
        //                 Primitive::I128 => ("I128", "i128"),
        //                 Primitive::Bool => ("Bool", "bool"),
        //                 Primitive::F32 => ("F32", "f32"),
        //                 Primitive::F64 => ("F64", "f64"),
        //             };

        //             scope.new_type_alias(from, to);
        //             self.type_mapping.insert(decl.to_owned(), from.to_string());
        //         },
        //         Definition::Mapping { key, value } => {
        //             todo!()
        //         }
        //         Definition::Sequence { decl: seq_decl } => {
        //             if decl == "String" && seq_decl == "Char" {
        //                 // Do nothing as Rust has a native string type.
        //             } else {
        //                 // type_mapping.contains_key(key)
        //                 todo!()
        //             }
        //         },
        //         Definition::FixedSequence { length, decl } => {
        //             // self.type_mapping.get(decl).unwrap_or_else(|| panic!("Missing type mapping
        // for {}", decl));             todo!()
        //         },
        //         Definition::Tuple { items } => {
        //             for item in items {

        //                 // todo!("handle {mapped_type_name}")
        //             }
        //         }
        //         Definition::Enum { items } => todo!(),
        //         Definition::Struct { items } => todo!(),
        //     }
        // }

        scope.to_string()
    }

    fn process_def(
        &mut self,
        scope: &mut Scope,
        def_index: usize,
        decl: &Declaration,
        def: &Definition,
    ) {
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn it_works() -> Result<(), std::io::Error> {
        let mut schema = Codegen::from_file("/tmp/cep18_schema.json").unwrap();
        let mut code = schema.gen();
        code.insert_str(
            0,
            "#![allow(dead_code, unused_variables, non_camel_case_types)]",
        );

        code += r#"fn main() {}"#;

        fs::write("/tmp/test.rs", &code)?;

        let t = trybuild::TestCases::new();
        t.pass("/tmp/test.rs");
        // eprintln!("")
        Ok(())
    }
}
