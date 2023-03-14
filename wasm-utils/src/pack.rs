use super::{gas::update_call_index, TargetRuntime};
use core::fmt;
use parity_wasm::{
    builder,
    elements::{
        self, DataSection, DataSegment, External, ImportCountType, InitExpr, Instruction, Internal,
        Section,
    },
};

/// Pack error.
///
/// Pack has number of assumptions of passed module structure.
/// When they are violated, pack_instance returns one of these.
#[derive(Debug)]
pub enum Error {
    MalformedModule,
    NoTypeSection,
    NoExportSection,
    NoCodeSection,
    InvalidCreateSignature(&'static str),
    NoCreateSymbol(&'static str),
    InvalidCreateMember(&'static str),
    NoImportSection,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        match *self {
            Error::MalformedModule => write!(f, "Module internal references are inconsistent"),
            Error::NoTypeSection => write!(f, "No type section in the module"),
            Error::NoExportSection => write!(f, "No export section in the module"),
            Error::NoCodeSection => write!(f, "No code section inthe module"),
            Error::InvalidCreateSignature(sym) => write!(
                f,
                "Exported symbol `{}` has invalid signature, should be () -> ()",
                sym
            ),
            Error::InvalidCreateMember(sym) => {
                write!(f, "Exported symbol `{}` should be a function", sym)
            }
            Error::NoCreateSymbol(sym) => write!(f, "No exported `{}` symbol", sym),
            Error::NoImportSection => write!(f, "No import section in the module"),
        }
    }
}

/// If a pwasm module has an exported function matching "create" symbol we want to pack it into
/// "constructor". `raw_module` is the actual contract code
/// `ctor_module` is the constructor which should return `raw_module`
pub fn pack_instance(
    raw_module: Vec<u8>,
    mut ctor_module: elements::Module,
    target: &TargetRuntime,
) -> Result<elements::Module, Error> {
    // Total number of constructor module import functions
    let ctor_import_functions = ctor_module
        .import_section()
        .map(|x| x.functions())
        .unwrap_or(0);

    // We need to find an internal ID of function which is exported as `symbols().create`
    // in order to find it in the Code section of the module
    let mut create_func_id = {
        let found_entry = ctor_module
            .export_section()
            .ok_or(Error::NoExportSection)?
            .entries()
            .iter()
            .find(|entry| target.symbols().create == entry.field())
            .ok_or_else(|| Error::NoCreateSymbol(target.symbols().create))?;

        let function_index: usize = match found_entry.internal() {
            Internal::Function(index) => *index as usize,
            _ => return Err(Error::InvalidCreateMember(target.symbols().create)),
        };

        // Calculates a function index within module's function section
        let function_internal_index = function_index - ctor_import_functions;

        // Constructor should be of signature `func()` (void), fail otherwise
        let type_id = ctor_module
            .function_section()
            .ok_or(Error::NoCodeSection)?
            .entries()
            .get(function_index - ctor_import_functions)
            .ok_or(Error::MalformedModule)?
            .type_ref();

        let elements::Type::Function(func) = ctor_module
            .type_section()
            .ok_or(Error::NoTypeSection)?
            .types()
            .get(type_id as usize)
            .ok_or(Error::MalformedModule)?;

        // Deploy should have no arguments and also should return nothing
        if !func.params().is_empty() {
            return Err(Error::InvalidCreateSignature(target.symbols().create));
        }
        if func.return_type().is_some() {
            return Err(Error::InvalidCreateSignature(target.symbols().create));
        }

        function_internal_index
    };

    let ret_function_id = {
        let mut id = 0;
        let mut found = false;
        for entry in ctor_module
            .import_section()
            .ok_or(Error::NoImportSection)?
            .entries()
            .iter()
        {
            if let External::Function(_) = *entry.external() {
                if entry.field() == target.symbols().ret {
                    found = true;
                    break;
                } else {
                    id += 1;
                }
            }
        }
        if !found {
            let mut mbuilder = builder::from_module(ctor_module);
            let import_sig = mbuilder
                .push_signature(builder::signature().param().i32().param().i32().build_sig());

            mbuilder.push_import(
                builder::import()
                    .module("env")
                    .field(target.symbols().ret)
                    .external()
                    .func(import_sig)
                    .build(),
            );

            ctor_module = mbuilder.build();

            let ret_func = ctor_module.import_count(ImportCountType::Function) as u32 - 1;

            for section in ctor_module.sections_mut() {
                match section {
                    elements::Section::Code(code_section) => {
                        for func_body in code_section.bodies_mut() {
                            update_call_index(func_body.code_mut(), ret_func);
                        }
                    }
                    elements::Section::Export(export_section) => {
                        for export in export_section.entries_mut() {
                            if let elements::Internal::Function(func_index) = export.internal_mut()
                            {
                                if *func_index >= ret_func {
                                    *func_index += 1
                                }
                            }
                        }
                    }
                    elements::Section::Element(elements_section) => {
                        for segment in elements_section.entries_mut() {
                            // update all indirect call addresses initial values
                            for func_index in segment.members_mut() {
                                if *func_index >= ret_func {
                                    *func_index += 1
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            create_func_id += 1;
            ret_func
        } else {
            id
        }
    };

    // If new function is put in ctor module, it will have this callable index
    let last_function_index = ctor_module.functions_space();

    // We ensure here that module has the DataSection
    if false
        == ctor_module
            .sections()
            .iter()
            .any(|section| matches!(*section, Section::Data(_)))
    {
        // DataSection has to be the last non-custom section according the to the spec
        ctor_module
            .sections_mut()
            .push(Section::Data(DataSection::with_entries(vec![])));
    }

    // Code data address is an address where we put the contract's code (raw_module)
    let mut code_data_address = 0i32;

    for section in ctor_module.sections_mut() {
        if let Section::Data(data_section) = section {
            let (index, offset) = if let Some(entry) = data_section.entries().iter().last() {
                let init_expr = entry
                    .offset()
                    .as_ref()
                    .expect("parity-wasm is compiled without bulk-memory operations")
                    .code();
                if let Instruction::I32Const(offst) = init_expr[0] {
                    let len = entry.value().len() as i32;
                    (entry.index(), offst + (len + 4) - len % 4)
                } else {
                    (0, 0)
                }
            } else {
                (0, 0)
            };
            let code_data = DataSegment::new(
                index,
                Some(InitExpr::new(vec![
                    Instruction::I32Const(offset),
                    Instruction::End,
                ])),
                raw_module.clone(),
            );
            data_section.entries_mut().push(code_data);
            code_data_address = offset;
        }
    }

    let mut new_module = builder::from_module(ctor_module)
        .function()
        .signature()
        .build()
        .body()
        .with_instructions(elements::Instructions::new(vec![
            Instruction::Call((create_func_id + ctor_import_functions) as u32),
            Instruction::I32Const(code_data_address),
            Instruction::I32Const(raw_module.len() as i32),
            Instruction::Call(ret_function_id),
            Instruction::End,
        ]))
        .build()
        .build()
        .build();

    for section in new_module.sections_mut() {
        if let Section::Export(export_section) = section {
            for entry in export_section.entries_mut().iter_mut() {
                if target.symbols().create == entry.field() {
                    // change `create` symbol export name into default `call` symbol name.
                    *entry.field_mut() = target.symbols().call.to_owned();
                    *entry.internal_mut() =
                        elements::Internal::Function(last_function_index as u32);
                }
            }
        }
    }

    Ok(new_module)
}

#[cfg(test)]
mod test {
    use super::{super::optimize, *};
    use parity_wasm::builder;

    fn test_packer(mut module: elements::Module, target_runtime: &TargetRuntime) {
        let mut ctor_module = module.clone();
        optimize(&mut module, vec![target_runtime.symbols().call])
            .expect("Optimizer to finish without errors");
        optimize(&mut ctor_module, vec![target_runtime.symbols().create])
            .expect("Optimizer to finish without errors");

        let raw_module = parity_wasm::serialize(module).unwrap();
        let ctor_module =
            pack_instance(raw_module.clone(), ctor_module, target_runtime).expect("Packing failed");

        let data_section = ctor_module
            .data_section()
            .expect("Packed module has to have a data section");
        let data_segment = data_section
            .entries()
            .iter()
            .last()
            .expect("Packed module has to have a data section with at least one entry");
        assert!(
            data_segment.value() == AsRef::<[u8]>::as_ref(&raw_module),
            "Last data segment should be equal to the raw module"
        );
    }

    #[test]
    fn no_data_section() {
        let target_runtime = TargetRuntime::pwasm();

        test_packer(
            builder::module()
                .import()
                .module("env")
                .field("memory")
                .external()
                .memory(1_u32, Some(1_u32))
                .build()
                .function()
                .signature()
                .params()
                .i32()
                .i32()
                .build()
                .build()
                .body()
                .build()
                .build()
                .function()
                .signature()
                .build()
                .body()
                .with_instructions(elements::Instructions::new(vec![
                    elements::Instruction::End,
                ]))
                .build()
                .build()
                .function()
                .signature()
                .build()
                .body()
                .with_instructions(elements::Instructions::new(vec![
                    elements::Instruction::End,
                ]))
                .build()
                .build()
                .export()
                .field(target_runtime.symbols().call)
                .internal()
                .func(1)
                .build()
                .export()
                .field(target_runtime.symbols().create)
                .internal()
                .func(2)
                .build()
                .build(),
            &target_runtime,
        );
    }

    #[test]
    fn with_data_section() {
        let target_runtime = TargetRuntime::pwasm();

        test_packer(
            builder::module()
                .import()
                .module("env")
                .field("memory")
                .external()
                .memory(1_u32, Some(1_u32))
                .build()
                .data()
                .offset(elements::Instruction::I32Const(16))
                .value(vec![0u8])
                .build()
                .function()
                .signature()
                .params()
                .i32()
                .i32()
                .build()
                .build()
                .body()
                .build()
                .build()
                .function()
                .signature()
                .build()
                .body()
                .with_instructions(elements::Instructions::new(vec![
                    elements::Instruction::End,
                ]))
                .build()
                .build()
                .function()
                .signature()
                .build()
                .body()
                .with_instructions(elements::Instructions::new(vec![
                    elements::Instruction::End,
                ]))
                .build()
                .build()
                .export()
                .field(target_runtime.symbols().call)
                .internal()
                .func(1)
                .build()
                .export()
                .field(target_runtime.symbols().create)
                .internal()
                .func(2)
                .build()
                .build(),
            &target_runtime,
        );
    }
}
