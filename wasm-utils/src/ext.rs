use crate::optimizer::{export_section, import_section};
use byteorder::{ByteOrder, LittleEndian};
use parity_wasm::{builder, elements};

type Insertion = (usize, u32, u32, String);

pub fn update_call_index(
    instructions: &mut elements::Instructions,
    original_imports: usize,
    inserts: &[Insertion],
) {
    use parity_wasm::elements::Instruction::*;
    for instruction in instructions.elements_mut().iter_mut() {
        if let Call(call_index) = instruction {
            if let Some(pos) = inserts.iter().position(|x| x.1 == *call_index) {
                *call_index = (original_imports + pos) as u32;
            } else if *call_index as usize > original_imports {
                *call_index += inserts.len() as u32;
            }
        }
    }
}

pub fn memory_section(module: &mut elements::Module) -> Option<&mut elements::MemorySection> {
    for section in module.sections_mut() {
        if let elements::Section::Memory(sect) = section {
            return Some(sect);
        }
    }
    None
}

pub fn externalize_mem(
    mut module: elements::Module,
    adjust_pages: Option<u32>,
    max_pages: u32,
) -> elements::Module {
    let mut entry = memory_section(&mut module)
        .expect("Memory section to exist")
        .entries_mut()
        .pop()
        .expect("Own memory entry to exist in memory section");

    if let Some(adjust_pages) = adjust_pages {
        assert!(adjust_pages <= max_pages);
        entry = elements::MemoryType::new(adjust_pages, Some(max_pages));
    }

    if entry.limits().maximum().is_none() {
        entry = elements::MemoryType::new(entry.limits().initial(), Some(max_pages));
    }

    let mut builder = builder::from_module(module);
    builder.push_import(elements::ImportEntry::new(
        "env".to_owned(),
        "memory".to_owned(),
        elements::External::Memory(entry),
    ));

    builder.build()
}

fn foreach_public_func_name<F>(mut module: elements::Module, f: F) -> elements::Module
where
    F: Fn(&mut String),
{
    if let Some(section) = import_section(&mut module) {
        for entry in section.entries_mut() {
            if let elements::External::Function(_) = *entry.external() {
                f(entry.field_mut())
            }
        }
    }

    if let Some(section) = export_section(&mut module) {
        for entry in section.entries_mut() {
            if let elements::Internal::Function(_) = *entry.internal() {
                f(entry.field_mut())
            }
        }
    }

    module
}

pub fn underscore_funcs(module: elements::Module) -> elements::Module {
    foreach_public_func_name(module, |n| n.insert(0, '_'))
}

pub fn ununderscore_funcs(module: elements::Module) -> elements::Module {
    foreach_public_func_name(module, |n| {
        n.remove(0);
    })
}

pub fn shrink_unknown_stack(
    mut module: elements::Module,
    // for example, `shrink_amount = (1MB - 64KB)` will limit stack to 64KB
    shrink_amount: u32,
) -> (elements::Module, u32) {
    let mut new_stack_top = 0;
    for section in module.sections_mut() {
        match section {
            elements::Section::Data(data_section) => {
                for data_segment in data_section.entries_mut() {
                    if *data_segment
                        .offset()
                        .as_ref()
                        .expect("parity-wasm is compiled without bulk-memory operations")
                        .code()
                        == [
                            elements::Instruction::I32Const(4),
                            elements::Instruction::End,
                        ]
                    {
                        assert_eq!(data_segment.value().len(), 4);
                        let current_val = LittleEndian::read_u32(data_segment.value());
                        let new_val = current_val - shrink_amount;
                        LittleEndian::write_u32(data_segment.value_mut(), new_val);
                        new_stack_top = new_val;
                    }
                }
            }
            _ => continue,
        }
    }
    (module, new_stack_top)
}

pub fn externalize(module: elements::Module, replaced_funcs: Vec<&str>) -> elements::Module {
    // Save import functions number for later
    let import_funcs_total = module
        .import_section()
        .expect("Import section to exist")
        .entries()
        .iter()
        .filter(|e| matches!(e.external(), &elements::External::Function(_)))
        .count();

    // First, we find functions indices that are to be rewired to externals
    //   Triple is (function_index (callable), type_index, function_name)
    let mut replaces: Vec<Insertion> = replaced_funcs
        .into_iter()
        .filter_map(|f| {
            let export = module
                .export_section()
                .expect("Export section to exist")
                .entries()
                .iter()
                .enumerate()
                .find(|&(_, entry)| entry.field() == f)
                .expect("All functions of interest to exist");

            if let elements::Internal::Function(func_idx) = *export.1.internal() {
                let type_ref = module
                    .function_section()
                    .expect("Functions section to exist")
                    .entries()[func_idx as usize - import_funcs_total]
                    .type_ref();

                Some((export.0, func_idx, type_ref, export.1.field().to_owned()))
            } else {
                None
            }
        })
        .collect();

    replaces.sort_by_key(|e| e.0);

    // Second, we duplicate them as import definitions
    let mut mbuilder = builder::from_module(module);
    for (_, _, type_ref, field) in replaces.iter() {
        mbuilder.push_import(
            builder::import()
                .module("env")
                .field(field)
                .external()
                .func(*type_ref)
                .build(),
        );
    }

    // Back to mutable access
    let mut module = mbuilder.build();

    // Third, rewire all calls to imported functions and update all other calls indices
    for section in module.sections_mut() {
        match section {
            elements::Section::Code(code_section) => {
                for func_body in code_section.bodies_mut() {
                    update_call_index(func_body.code_mut(), import_funcs_total, &replaces);
                }
            }
            elements::Section::Export(export_section) => {
                for export in export_section.entries_mut() {
                    if let elements::Internal::Function(func_index) = export.internal_mut() {
                        if *func_index >= import_funcs_total as u32 {
                            *func_index += replaces.len() as u32;
                        }
                    }
                }
            }
            elements::Section::Element(elements_section) => {
                for segment in elements_section.entries_mut() {
                    // update all indirect call addresses initial values
                    for func_index in segment.members_mut() {
                        if *func_index >= import_funcs_total as u32 {
                            *func_index += replaces.len() as u32;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    module
}
