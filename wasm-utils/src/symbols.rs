use parity_wasm::elements;
use std::collections::HashSet as Set;

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Copy, Clone, Debug)]
pub enum Symbol {
    Type(usize),
    Import(usize),
    Global(usize),
    Function(usize),
    Export(usize),
}

pub fn resolve_function(module: &elements::Module, index: u32) -> Symbol {
    let mut functions = 0;
    if let Some(import_section) = module.import_section() {
        for (item_index, item) in import_section.entries().iter().enumerate() {
            if let elements::External::Function(_) = item.external() {
                if functions == index {
                    return Symbol::Import(item_index);
                }
                functions += 1;
            }
        }
    }

    Symbol::Function(index as usize - functions as usize)
}

pub fn resolve_global(module: &elements::Module, index: u32) -> Symbol {
    let mut globals = 0;
    if let Some(import_section) = module.import_section() {
        for (item_index, item) in import_section.entries().iter().enumerate() {
            if let elements::External::Global(_) = item.external() {
                if globals == index {
                    return Symbol::Import(item_index);
                }
                globals += 1;
            }
        }
    }

    Symbol::Global(index as usize - globals as usize)
}

pub fn push_code_symbols(
    module: &elements::Module,
    instructions: &[elements::Instruction],
    dest: &mut Vec<Symbol>,
) {
    use parity_wasm::elements::Instruction::*;

    for instruction in instructions {
        match instruction {
            &Call(idx) => {
                dest.push(resolve_function(module, idx));
            }
            &CallIndirect(idx, _) => {
                dest.push(Symbol::Type(idx as usize));
            }
            &GetGlobal(idx) | &SetGlobal(idx) => dest.push(resolve_global(module, idx)),
            _ => {}
        }
    }
}

pub fn expand_symbols(module: &elements::Module, set: &mut Set<Symbol>) {
    use Symbol::*;

    // symbols that were already processed
    let mut stop: Set<Symbol> = Set::new();
    let mut fringe = set.iter().cloned().collect::<Vec<Symbol>>();
    loop {
        let next = match fringe.pop() {
            Some(s) if stop.contains(&s) => {
                continue;
            }
            Some(s) => s,
            _ => {
                break;
            }
        };
        log::trace!("Processing symbol {:?}", next);

        match next {
            Export(idx) => {
                let entry = &module
                    .export_section()
                    .expect("Export section to exist")
                    .entries()[idx];
                match entry.internal() {
                    elements::Internal::Function(func_idx) => {
                        let symbol = resolve_function(module, *func_idx);
                        if !stop.contains(&symbol) {
                            fringe.push(symbol);
                        }
                        set.insert(symbol);
                    }
                    elements::Internal::Global(global_idx) => {
                        let symbol = resolve_global(module, *global_idx);
                        if !stop.contains(&symbol) {
                            fringe.push(symbol);
                        }
                        set.insert(symbol);
                    }
                    _ => {}
                }
            }
            Import(idx) => {
                let entry = &module
                    .import_section()
                    .expect("Import section to exist")
                    .entries()[idx];
                if let elements::External::Function(type_idx) = entry.external() {
                    let type_symbol = Symbol::Type(*type_idx as usize);
                    if !stop.contains(&type_symbol) {
                        fringe.push(type_symbol);
                    }
                    set.insert(type_symbol);
                }
            }
            Function(idx) => {
                let body = &module
                    .code_section()
                    .expect("Code section to exist")
                    .bodies()[idx];
                let mut code_symbols = Vec::new();
                push_code_symbols(module, body.code().elements(), &mut code_symbols);
                for symbol in code_symbols.drain(..) {
                    if !stop.contains(&symbol) {
                        fringe.push(symbol);
                    }
                    set.insert(symbol);
                }

                let signature = &module
                    .function_section()
                    .expect("Functions section to exist")
                    .entries()[idx];
                let type_symbol = Symbol::Type(signature.type_ref() as usize);
                if !stop.contains(&type_symbol) {
                    fringe.push(type_symbol);
                }
                set.insert(type_symbol);
            }
            Global(idx) => {
                let entry = &module
                    .global_section()
                    .expect("Global section to exist")
                    .entries()[idx];
                let mut code_symbols = Vec::new();
                push_code_symbols(module, entry.init_expr().code(), &mut code_symbols);
                for symbol in code_symbols.drain(..) {
                    if !stop.contains(&symbol) {
                        fringe.push(symbol);
                    }
                    set.insert(symbol);
                }
            }
            _ => {}
        }

        stop.insert(next);
    }
}
