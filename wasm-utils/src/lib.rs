#![allow(clippy::bool_comparison)]

pub mod rules;

mod build;
mod export_globals;
mod ext;
mod gas;
mod graph;
mod optimizer;
mod pack;
mod ref_list;
mod runtime_type;
mod symbols;

pub mod stack_height;

pub use build::{build, Error as BuildError, SourceTarget};
pub use export_globals::export_mutable_globals;
pub use ext::{
    externalize, externalize_mem, shrink_unknown_stack, underscore_funcs, ununderscore_funcs,
};
pub use gas::inject_gas_counter;
pub use graph::{generate as graph_generate, parse as graph_parse, Module};
pub use optimizer::{optimize, Error as OptimizerError};
pub use pack::{pack_instance, Error as PackingError};
pub use ref_list::{DeleteTransaction, Entry, EntryRef, RefList};
pub use runtime_type::inject_runtime_type;

pub struct TargetSymbols {
    pub create: &'static str,
    pub call: &'static str,
    pub ret: &'static str,
}

pub enum TargetRuntime {
    Substrate(TargetSymbols),
    PWasm(TargetSymbols),
}

impl TargetRuntime {
    pub fn substrate() -> TargetRuntime {
        TargetRuntime::Substrate(TargetSymbols {
            create: "deploy",
            call: "call",
            ret: "ext_return",
        })
    }

    pub fn pwasm() -> TargetRuntime {
        TargetRuntime::PWasm(TargetSymbols {
            create: "deploy",
            call: "call",
            ret: "ret",
        })
    }

    pub fn symbols(&self) -> &TargetSymbols {
        match self {
            TargetRuntime::Substrate(s) => s,
            TargetRuntime::PWasm(s) => s,
        }
    }
}
