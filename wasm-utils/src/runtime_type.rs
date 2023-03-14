use byteorder::{ByteOrder, LittleEndian};
use elements::{
    ExportEntry, External, GlobalEntry, GlobalType, InitExpr, Instruction, Internal, Module,
    ValueType,
};
use parity_wasm::{builder, elements};

pub fn inject_runtime_type(module: Module, runtime_type: [u8; 4], runtime_version: u32) -> Module {
    let runtime_type: u32 = LittleEndian::read_u32(&runtime_type);
    let globals_count: u32 = match module.global_section() {
        Some(section) => section.entries().len() as u32,
        None => 0,
    };
    let imported_globals_count: u32 = match module.import_section() {
        Some(section) => section
            .entries()
            .iter()
            .filter(|e| matches!(*e.external(), External::Global(_)))
            .count() as u32,
        None => 0,
    };
    let total_globals_count: u32 = globals_count + imported_globals_count;

    builder::from_module(module)
        .with_global(GlobalEntry::new(
            GlobalType::new(ValueType::I32, false),
            InitExpr::new(vec![
                Instruction::I32Const(runtime_type as i32),
                Instruction::End,
            ]),
        ))
        .with_export(ExportEntry::new(
            "RUNTIME_TYPE".into(),
            Internal::Global(total_globals_count),
        ))
        .with_global(GlobalEntry::new(
            GlobalType::new(ValueType::I32, false),
            InitExpr::new(vec![
                Instruction::I32Const(runtime_version as i32),
                Instruction::End,
            ]),
        ))
        .with_export(ExportEntry::new(
            "RUNTIME_VERSION".into(),
            Internal::Global(total_globals_count + 1),
        ))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_injects() {
        let mut module = builder::module()
            .with_global(GlobalEntry::new(
                GlobalType::new(ValueType::I32, false),
                InitExpr::new(vec![Instruction::I32Const(42_i32)]),
            ))
            .build();
        let mut runtime_type: [u8; 4] = Default::default();
        runtime_type.copy_from_slice(b"emcc");
        module = inject_runtime_type(module, runtime_type, 1);
        let global_section = module.global_section().expect("Global section expected");
        assert_eq!(3, global_section.entries().len());
        let export_section = module.export_section().expect("Export section expected");
        assert!(export_section
            .entries()
            .iter()
            .any(|e| e.field() == "RUNTIME_TYPE"));
        assert!(export_section
            .entries()
            .iter()
            .any(|e| e.field() == "RUNTIME_VERSION"));
    }
}
