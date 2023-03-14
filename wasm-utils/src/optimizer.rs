use crate::symbols::{expand_symbols, push_code_symbols, resolve_function, Symbol};
use log::trace;
use parity_wasm::elements;
use std::{collections::HashSet as Set, mem};

#[derive(Debug)]
pub enum Error {
    /// Since optimizer starts with export entries, export
    ///   section is supposed to exist.
    NoExportSection,
}

pub fn optimize(
    module: &mut elements::Module, // Module to optimize
    used_exports: Vec<&str>,       // List of only exports that will be usable after optimization
) -> Result<(), Error> {
    // WebAssembly exports optimizer
    // Motivation: emscripten compiler backend compiles in many unused exports
    //   which in turn compile in unused imports and leaves unused functions

    // try to parse name section
    let module_temp = mem::take(module);
    let module_temp = module_temp
        .parse_names()
        .unwrap_or_else(|(_err, module)| module);
    *module = module_temp;

    // Algo starts from the top, listing all items that should stay
    let mut stay = Set::new();
    for (index, entry) in module
        .export_section()
        .ok_or(Error::NoExportSection)?
        .entries()
        .iter()
        .enumerate()
    {
        if used_exports.iter().any(|e| *e == entry.field()) {
            stay.insert(Symbol::Export(index));
        }
    }

    // If there is start function in module, it should stary
    module
        .start_section()
        .map(|ss| stay.insert(resolve_function(module, ss)));

    // All symbols used in data/element segments are also should be preserved
    let mut init_symbols = Vec::new();
    if let Some(data_section) = module.data_section() {
        for segment in data_section.entries() {
            push_code_symbols(
                module,
                segment
                    .offset()
                    .as_ref()
                    .expect("parity-wasm is compiled without bulk-memory operations")
                    .code(),
                &mut init_symbols,
            );
        }
    }
    if let Some(elements_section) = module.elements_section() {
        for segment in elements_section.entries() {
            push_code_symbols(
                module,
                segment
                    .offset()
                    .as_ref()
                    .expect("parity-wasm is compiled without bulk-memory operations")
                    .code(),
                &mut init_symbols,
            );
            for func_index in segment.members() {
                stay.insert(resolve_function(module, *func_index));
            }
        }
    }
    for symbol in init_symbols.drain(..) {
        stay.insert(symbol);
    }

    // Call function which will traverse the list recursively, filling stay with all symbols
    // that are already used by those which already there
    expand_symbols(module, &mut stay);

    for symbol in stay.iter() {
        trace!("symbol to stay: {:?}", symbol);
    }

    // Keep track of referreable symbols to rewire calls/globals
    let mut eliminated_funcs = Vec::new();
    let mut eliminated_globals = Vec::new();
    let mut eliminated_types = Vec::new();

    // First, iterate through types
    let mut index = 0;
    let mut old_index = 0;

    {
        loop {
            if type_section(module)
                .map(|section| section.types_mut().len())
                .unwrap_or(0)
                == index
            {
                break;
            }

            if stay.contains(&Symbol::Type(old_index)) {
                index += 1;
            } else {
                type_section(module)
					.expect("If type section does not exists, the loop will break at the beginning of first iteration")
					.types_mut().remove(index);
                eliminated_types.push(old_index);
                trace!("Eliminated type({})", old_index);
            }
            old_index += 1;
        }
    }

    // Second, iterate through imports
    let mut top_funcs = 0;
    let mut top_globals = 0;
    index = 0;
    old_index = 0;

    if let Some(imports) = import_section(module) {
        loop {
            let mut remove = false;
            match imports.entries()[index].external() {
                elements::External::Function(_) => {
                    if stay.contains(&Symbol::Import(old_index)) {
                        index += 1;
                    } else {
                        remove = true;
                        eliminated_funcs.push(top_funcs);
                        trace!(
                            "Eliminated import({}) func({}, {})",
                            old_index,
                            top_funcs,
                            imports.entries()[index].field()
                        );
                    }
                    top_funcs += 1;
                }
                elements::External::Global(_) => {
                    if stay.contains(&Symbol::Import(old_index)) {
                        index += 1;
                    } else {
                        remove = true;
                        eliminated_globals.push(top_globals);
                        trace!(
                            "Eliminated import({}) global({}, {})",
                            old_index,
                            top_globals,
                            imports.entries()[index].field()
                        );
                    }
                    top_globals += 1;
                }
                _ => {
                    index += 1;
                }
            }
            if remove {
                imports.entries_mut().remove(index);
            }

            old_index += 1;

            if index == imports.entries().len() {
                break;
            }
        }
    }

    // Third, iterate through globals
    if let Some(globals) = global_section(module) {
        index = 0;
        old_index = 0;

        loop {
            if globals.entries_mut().len() == index {
                break;
            }
            if stay.contains(&Symbol::Global(old_index)) {
                index += 1;
            } else {
                globals.entries_mut().remove(index);
                eliminated_globals.push(top_globals + old_index);
                trace!("Eliminated global({})", top_globals + old_index);
            }
            old_index += 1;
        }
    }

    // Forth, delete orphaned functions
    if function_section(module).is_some() && code_section(module).is_some() {
        index = 0;
        old_index = 0;

        loop {
            if function_section(module)
                .expect("Functons section to exist")
                .entries_mut()
                .len()
                == index
            {
                break;
            }
            if stay.contains(&Symbol::Function(old_index)) {
                index += 1;
            } else {
                function_section(module)
                    .expect("Functons section to exist")
                    .entries_mut()
                    .remove(index);
                code_section(module)
                    .expect("Code section to exist")
                    .bodies_mut()
                    .remove(index);

                eliminated_funcs.push(top_funcs + old_index);
                trace!("Eliminated function({})", top_funcs + old_index);
            }
            old_index += 1;
        }
    }

    // Fifth, eliminate unused exports
    {
        let exports = export_section(module).ok_or(Error::NoExportSection)?;

        index = 0;
        old_index = 0;

        loop {
            if exports.entries_mut().len() == index {
                break;
            }
            if stay.contains(&Symbol::Export(old_index)) {
                index += 1;
            } else {
                trace!(
                    "Eliminated export({}, {})",
                    old_index,
                    exports.entries_mut()[index].field()
                );
                exports.entries_mut().remove(index);
            }
            old_index += 1;
        }
    }

    if !eliminated_globals.is_empty()
        || !eliminated_funcs.is_empty()
        || !eliminated_types.is_empty()
    {
        // Finaly, rewire all calls, globals references and types to the new indices
        //   (only if there is anything to do)
        // When sorting primitives sorting unstable is faster without any difference in result.
        eliminated_globals.sort_unstable();
        eliminated_funcs.sort_unstable();
        eliminated_types.sort_unstable();

        for section in module.sections_mut() {
            match section {
                elements::Section::Start(func_index) if !eliminated_funcs.is_empty() => {
                    let totalle = eliminated_funcs
                        .iter()
                        .take_while(|i| (**i as u32) < *func_index)
                        .count();
                    *func_index -= totalle as u32;
                }
                elements::Section::Function(function_section) if !eliminated_types.is_empty() => {
                    for func_signature in function_section.entries_mut() {
                        let totalle = eliminated_types
                            .iter()
                            .take_while(|i| (**i as u32) < func_signature.type_ref())
                            .count();
                        *func_signature.type_ref_mut() -= totalle as u32;
                    }
                }
                elements::Section::Import(import_section) if !eliminated_types.is_empty() => {
                    for import_entry in import_section.entries_mut() {
                        if let elements::External::Function(type_ref) = import_entry.external_mut()
                        {
                            let totalle = eliminated_types
                                .iter()
                                .take_while(|i| (**i as u32) < *type_ref)
                                .count();
                            *type_ref -= totalle as u32;
                        }
                    }
                }
                elements::Section::Code(code_section)
                    if !eliminated_globals.is_empty() || !eliminated_funcs.is_empty() =>
                {
                    for func_body in code_section.bodies_mut() {
                        if !eliminated_funcs.is_empty() {
                            update_call_index(func_body.code_mut(), &eliminated_funcs);
                        }
                        if !eliminated_globals.is_empty() {
                            update_global_index(
                                func_body.code_mut().elements_mut(),
                                &eliminated_globals,
                            )
                        }
                        if !eliminated_types.is_empty() {
                            update_type_index(func_body.code_mut(), &eliminated_types)
                        }
                    }
                }
                elements::Section::Export(export_section) => {
                    for export in export_section.entries_mut() {
                        match export.internal_mut() {
                            elements::Internal::Function(func_index) => {
                                let totalle = eliminated_funcs
                                    .iter()
                                    .take_while(|i| (**i as u32) < *func_index)
                                    .count();
                                *func_index -= totalle as u32;
                            }
                            elements::Internal::Global(global_index) => {
                                let totalle = eliminated_globals
                                    .iter()
                                    .take_while(|i| (**i as u32) < *global_index)
                                    .count();
                                *global_index -= totalle as u32;
                            }
                            _ => {}
                        }
                    }
                }
                elements::Section::Global(global_section) => {
                    for global_entry in global_section.entries_mut() {
                        update_global_index(
                            global_entry.init_expr_mut().code_mut(),
                            &eliminated_globals,
                        )
                    }
                }
                elements::Section::Data(data_section) => {
                    for segment in data_section.entries_mut() {
                        update_global_index(
                            segment
                                .offset_mut()
                                .as_mut()
                                .expect("parity-wasm is compiled without bulk-memory operations")
                                .code_mut(),
                            &eliminated_globals,
                        )
                    }
                }
                elements::Section::Element(elements_section) => {
                    for segment in elements_section.entries_mut() {
                        update_global_index(
                            segment
                                .offset_mut()
                                .as_mut()
                                .expect("parity-wasm is compiled without bulk-memory operations")
                                .code_mut(),
                            &eliminated_globals,
                        );
                        // update all indirect call addresses initial values
                        for func_index in segment.members_mut() {
                            let totalle = eliminated_funcs
                                .iter()
                                .take_while(|i| (**i as u32) < *func_index)
                                .count();
                            *func_index -= totalle as u32;
                        }
                    }
                }
                elements::Section::Name(name_section) => {
                    if let Some(func_name) = name_section.functions_mut() {
                        let mut func_name_map = mem::take(func_name.names_mut());
                        for index in &eliminated_funcs {
                            func_name_map.remove(*index as u32);
                        }
                        let updated_map = func_name_map
                            .into_iter()
                            .map(|(index, value)| {
                                let totalle = eliminated_funcs
                                    .iter()
                                    .take_while(|i| (**i as u32) < index)
                                    .count() as u32;
                                (index - totalle, value)
                            })
                            .collect();
                        *func_name.names_mut() = updated_map;
                    }

                    if let Some(local_name) = name_section.locals_mut() {
                        let mut local_names_map = mem::take(local_name.local_names_mut());
                        for index in &eliminated_funcs {
                            local_names_map.remove(*index as u32);
                        }
                        let updated_map = local_names_map
                            .into_iter()
                            .map(|(index, value)| {
                                let totalle = eliminated_funcs
                                    .iter()
                                    .take_while(|i| (**i as u32) < index)
                                    .count() as u32;
                                (index - totalle, value)
                            })
                            .collect();
                        *local_name.local_names_mut() = updated_map;
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

pub fn update_call_index(instructions: &mut elements::Instructions, eliminated_indices: &[usize]) {
    use parity_wasm::elements::Instruction::*;
    for instruction in instructions.elements_mut().iter_mut() {
        if let Call(call_index) = instruction {
            let totalle = eliminated_indices
                .iter()
                .take_while(|i| (**i as u32) < *call_index)
                .count();
            trace!(
                "rewired call {} -> call {}",
                *call_index,
                *call_index - totalle as u32
            );
            *call_index -= totalle as u32;
        }
    }
}

/// Updates global references considering the _ordered_ list of eliminated indices
pub fn update_global_index(
    instructions: &mut [elements::Instruction],
    eliminated_indices: &[usize],
) {
    use parity_wasm::elements::Instruction::*;
    for instruction in instructions.iter_mut() {
        match instruction {
            GetGlobal(index) | SetGlobal(index) => {
                let totalle = eliminated_indices
                    .iter()
                    .take_while(|i| (**i as u32) < *index)
                    .count();
                trace!(
                    "rewired global {} -> global {}",
                    *index,
                    *index - totalle as u32
                );
                *index -= totalle as u32;
            }
            _ => {}
        }
    }
}

/// Updates global references considering the _ordered_ list of eliminated indices
pub fn update_type_index(instructions: &mut elements::Instructions, eliminated_indices: &[usize]) {
    use parity_wasm::elements::Instruction::*;
    for instruction in instructions.elements_mut().iter_mut() {
        if let CallIndirect(call_index, _) = instruction {
            let totalle = eliminated_indices
                .iter()
                .take_while(|i| (**i as u32) < *call_index)
                .count();
            trace!(
                "rewired call_indrect {} -> call_indirect {}",
                *call_index,
                *call_index - totalle as u32
            );
            *call_index -= totalle as u32;
        }
    }
}

pub fn import_section(module: &mut elements::Module) -> Option<&mut elements::ImportSection> {
    for section in module.sections_mut() {
        if let elements::Section::Import(sect) = section {
            return Some(sect);
        }
    }
    None
}

pub fn global_section(module: &mut elements::Module) -> Option<&mut elements::GlobalSection> {
    for section in module.sections_mut() {
        if let elements::Section::Global(sect) = section {
            return Some(sect);
        }
    }
    None
}

pub fn function_section(module: &mut elements::Module) -> Option<&mut elements::FunctionSection> {
    for section in module.sections_mut() {
        if let elements::Section::Function(sect) = section {
            return Some(sect);
        }
    }
    None
}

pub fn code_section(module: &mut elements::Module) -> Option<&mut elements::CodeSection> {
    for section in module.sections_mut() {
        if let elements::Section::Code(sect) = section {
            return Some(sect);
        }
    }
    None
}

pub fn export_section(module: &mut elements::Module) -> Option<&mut elements::ExportSection> {
    for section in module.sections_mut() {
        if let elements::Section::Export(sect) = section {
            return Some(sect);
        }
    }
    None
}

pub fn type_section(module: &mut elements::Module) -> Option<&mut elements::TypeSection> {
    for section in module.sections_mut() {
        if let elements::Section::Type(sect) = section {
            return Some(sect);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use parity_wasm::{builder, elements};

    /// @spec 0
    /// Optimizer presumes that export section exists and contains
    /// all symbols passed as a second parameter. Since empty module
    /// obviously contains no export section, optimizer should return
    /// error on it.
    #[test]
    fn empty() {
        let mut module = builder::module().build();
        let result = optimize(&mut module, vec!["_call"]);

        assert!(result.is_err());
    }

    /// @spec 1
    /// Imagine the unoptimized module has two own functions, `_call` and `_random`
    /// and exports both of them in the export section. During optimization, the `_random`
    /// function should vanish completely, given we pass `_call` as the only function to stay
    /// in the module.
    #[test]
    fn minimal() {
        let mut module = builder::module()
            .function()
            .signature()
            .param()
            .i32()
            .build()
            .build()
            .function()
            .signature()
            .param()
            .i32()
            .param()
            .i32()
            .build()
            .build()
            .export()
            .field("_call")
            .internal()
            .func(0)
            .build()
            .export()
            .field("_random")
            .internal()
            .func(1)
            .build()
            .build();
        assert_eq!(
            module
                .export_section()
                .expect("export section to be generated")
                .entries()
                .len(),
            2
        );

        optimize(&mut module, vec!["_call"]).expect("optimizer to succeed");

        assert_eq!(
            1,
            module
                .export_section()
                .expect("export section to be generated")
                .entries()
                .len(),
            "There should only 1 (one) export entry in the optimized module"
        );

        assert_eq!(
            1,
            module
                .function_section()
                .expect("functions section to be generated")
                .entries()
                .len(),
            "There should 2 (two) functions in the optimized module"
        );
    }

    /// @spec 2
    /// Imagine there is one exported function in unoptimized module, `_call`, that we specify as
    /// the one to stay during the optimization. The code of this function uses global during
    /// the execution. This sayed global should survive the optimization.
    #[test]
    fn globals() {
        let mut module = builder::module()
            .global()
            .value_type()
            .i32()
            .build()
            .function()
            .signature()
            .param()
            .i32()
            .build()
            .body()
            .with_instructions(elements::Instructions::new(vec![
                elements::Instruction::GetGlobal(0),
                elements::Instruction::End,
            ]))
            .build()
            .build()
            .export()
            .field("_call")
            .internal()
            .func(0)
            .build()
            .build();

        optimize(&mut module, vec!["_call"]).expect("optimizer to succeed");

        assert_eq!(
			1,
			module.global_section().expect("global section to be generated").entries().len(),
			"There should 1 (one) global entry in the optimized module, since _call function uses it"
		);
    }

    /// @spec 2
    /// Imagine there is one exported function in unoptimized module, `_call`, that we specify as
    /// the one to stay during the optimization. The code of this function uses one global
    /// during the execution, but we have a bunch of other unused globals in the code. Last
    /// globals should not survive the optimization, while the former should.
    #[test]
    fn globals_2() {
        let mut module = builder::module()
            .global()
            .value_type()
            .i32()
            .build()
            .global()
            .value_type()
            .i64()
            .build()
            .global()
            .value_type()
            .f32()
            .build()
            .function()
            .signature()
            .param()
            .i32()
            .build()
            .body()
            .with_instructions(elements::Instructions::new(vec![
                elements::Instruction::GetGlobal(1),
                elements::Instruction::End,
            ]))
            .build()
            .build()
            .export()
            .field("_call")
            .internal()
            .func(0)
            .build()
            .build();

        optimize(&mut module, vec!["_call"]).expect("optimizer to succeed");

        assert_eq!(
			1,
			module.global_section().expect("global section to be generated").entries().len(),
			"There should 1 (one) global entry in the optimized module, since _call function uses only one"
		);
    }

    /// @spec 3
    /// Imagine the unoptimized module has two own functions, `_call` and `_random`
    /// and exports both of them in the export section. Function `_call` also calls `_random`
    /// in its function body. The optimization should kick `_random` function from the export
    /// section but preserve it's body.
    #[test]
    fn call_ref() {
        let mut module = builder::module()
            .function()
            .signature()
            .param()
            .i32()
            .build()
            .body()
            .with_instructions(elements::Instructions::new(vec![
                elements::Instruction::Call(1),
                elements::Instruction::End,
            ]))
            .build()
            .build()
            .function()
            .signature()
            .param()
            .i32()
            .param()
            .i32()
            .build()
            .build()
            .export()
            .field("_call")
            .internal()
            .func(0)
            .build()
            .export()
            .field("_random")
            .internal()
            .func(1)
            .build()
            .build();
        assert_eq!(
            module
                .export_section()
                .expect("export section to be generated")
                .entries()
                .len(),
            2
        );

        optimize(&mut module, vec!["_call"]).expect("optimizer to succeed");

        assert_eq!(
            1,
            module
                .export_section()
                .expect("export section to be generated")
                .entries()
                .len(),
            "There should only 1 (one) export entry in the optimized module"
        );

        assert_eq!(
            2,
            module
                .function_section()
                .expect("functions section to be generated")
                .entries()
                .len(),
            "There should 2 (two) functions in the optimized module"
        );
    }

    /// @spec 4
    /// Imagine the unoptimized module has an indirect call to function of type 1
    /// The type should persist so that indirect call would work
    #[test]
    fn call_indirect() {
        let mut module = builder::module()
            .function()
            .signature()
            .param()
            .i32()
            .param()
            .i32()
            .build()
            .build()
            .function()
            .signature()
            .param()
            .i32()
            .param()
            .i32()
            .param()
            .i32()
            .build()
            .build()
            .function()
            .signature()
            .param()
            .i32()
            .build()
            .body()
            .with_instructions(elements::Instructions::new(vec![
                elements::Instruction::CallIndirect(1, 0),
                elements::Instruction::End,
            ]))
            .build()
            .build()
            .export()
            .field("_call")
            .internal()
            .func(2)
            .build()
            .build();

        optimize(&mut module, vec!["_call"]).expect("optimizer to succeed");

        assert_eq!(
            2,
            module
                .type_section()
                .expect("type section to be generated")
                .types()
                .len(),
            "There should 2 (two) types left in the module, 1 for indirect call and one for _call"
        );

        let indirect_opcode = &module
            .code_section()
            .expect("code section to be generated")
            .bodies()[0]
            .code()
            .elements()[0];
        match *indirect_opcode {
            elements::Instruction::CallIndirect(0, 0) => {}
            _ => {
                panic!(
					"Expected call_indirect to use index 0 after optimization, since previois 0th was eliminated, but got {:?}",
					indirect_opcode
				);
            }
        }
    }
}
