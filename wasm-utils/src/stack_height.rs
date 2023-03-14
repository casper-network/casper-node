//! The pass that tries to make stack overflows deterministic, by introducing
//! an upper bound of the stack size.
//!
//! This pass introduces a global mutable variable to track stack height,
//! and instruments all calls with preamble and postamble.
//!
//! Stack height is increased prior the call. Otherwise, the check would
//! be made after the stack frame is allocated.
//!
//! The preamble is inserted before the call. It increments
//! the global stack height variable with statically determined "stack cost"
//! of the callee. If after the increment the stack height exceeds
//! the limit (specified by the `rules`) then execution traps.
//! Otherwise, the call is executed.
//!
//! The postamble is inserted after the call. The purpose of the postamble is to decrease
//! the stack height by the "stack cost" of the callee function.
//!
//! Note, that we can't instrument all possible ways to return from the function. The simplest
//! example would be a trap issued by the host function.
//! That means stack height global won't be equal to zero upon the next execution after such trap.
//!
//! # Thunks
//!
//! Because stack height is increased prior the call few problems arises:
//!
//! - Stack height isn't increased upon an entry to the first function, i.e. exported function.
//! - Start function is executed externally (similar to exported functions).
//! - It is statically unknown what function will be invoked in an indirect call.
//!
//! The solution for this problems is to generate a intermediate functions, called 'thunks', which
//! will increase before and decrease the stack height after the call to original function, and
//! then make exported function and table entries, start section to point to a corresponding thunks.
//!
//! # Stack cost
//!
//! Stack cost of the function is calculated as a sum of it's locals
//! and the maximal height of the value stack.
//!
//! All values are treated equally, as they have the same size.
//!
//! The rationale is that this makes it possible to use the following very naive wasm executor:
//!
//! - values are implemented by a union, so each value takes a size equal to the size of the largest
//!   possible value type this union can hold. (In MVP it is 8 bytes)
//! - each value from the value stack is placed on the native stack.
//! - each local variable and function argument is placed on the native stack.
//! - arguments pushed by the caller are copied into callee stack rather than shared between the
//!   frames.
//! - upon entry into the function entire stack frame is allocated.

use parity_wasm::{
    builder,
    elements::{self, Type},
};

/// Macro to generate preamble and postamble.
macro_rules! instrument_call {
    ($callee_idx: expr, $callee_stack_cost: expr, $stack_height_global_idx: expr, $stack_limit: expr) => {{
        use ::parity_wasm::elements::Instruction::*;
        [
            // stack_height += stack_cost(F)
            GetGlobal($stack_height_global_idx),
            I32Const($callee_stack_cost),
            I32Add,
            SetGlobal($stack_height_global_idx),
            // if stack_counter > LIMIT: unreachable
            GetGlobal($stack_height_global_idx),
            I32Const($stack_limit as i32),
            I32GtU,
            If(elements::BlockType::NoResult),
            Unreachable,
            End,
            // Original call
            Call($callee_idx),
            // stack_height -= stack_cost(F)
            GetGlobal($stack_height_global_idx),
            I32Const($callee_stack_cost),
            I32Sub,
            SetGlobal($stack_height_global_idx),
        ]
    }};
}

mod max_height;
mod thunk;

/// Error that occured during processing the module.
///
/// This means that the module is invalid.
#[derive(Debug)]
pub struct Error(String);

pub(crate) struct Context {
    stack_height_global_idx: u32,
    func_stack_costs: Vec<u32>,
    stack_limit: u32,
}

impl Context {
    /// Returns index in a global index space of a stack_height global variable.
    fn stack_height_global_idx(&self) -> u32 {
        self.stack_height_global_idx
    }

    /// Returns `stack_cost` for `func_idx`.
    fn stack_cost(&self, func_idx: u32) -> Option<u32> {
        self.func_stack_costs.get(func_idx as usize).cloned()
    }

    /// Returns stack limit specified by the rules.
    fn stack_limit(&self) -> u32 {
        self.stack_limit
    }
}

/// Instrument a module with stack height limiter.
///
/// See module-level documentation for more details.
///
/// # Errors
///
/// Returns `Err` if module is invalid and can't be
pub fn inject_limiter(
    mut module: elements::Module,
    stack_limit: u32,
) -> Result<elements::Module, Error> {
    let mut ctx = Context {
        stack_height_global_idx: generate_stack_height_global(&mut module),
        func_stack_costs: compute_stack_costs(&module)?,
        stack_limit,
    };

    instrument_functions(&mut ctx, &mut module)?;
    let module = thunk::generate_thunks(&mut ctx, module)?;

    Ok(module)
}

/// Generate a new global that will be used for tracking current stack height.
fn generate_stack_height_global(module: &mut elements::Module) -> u32 {
    let global_entry = builder::global()
        .value_type()
        .i32()
        .mutable()
        .init_expr(elements::Instruction::I32Const(0))
        .build();

    // Try to find an existing global section.
    for section in module.sections_mut() {
        if let elements::Section::Global(gs) = section {
            gs.entries_mut().push(global_entry);
            return (gs.entries().len() as u32) - 1;
        }
    }

    // Existing section not found, create one!
    module.sections_mut().push(elements::Section::Global(
        elements::GlobalSection::with_entries(vec![global_entry]),
    ));
    0
}

/// Calculate stack costs for all functions.
///
/// Returns a vector with a stack cost for each function, including imports.
fn compute_stack_costs(module: &elements::Module) -> Result<Vec<u32>, Error> {
    let func_imports = module.import_count(elements::ImportCountType::Function);

    // TODO: optimize!
    (0..module.functions_space())
        .map(|func_idx| {
            if func_idx < func_imports {
                // We can't calculate stack_cost of the import functions.
                Ok(0)
            } else {
                compute_stack_cost(func_idx as u32, module)
            }
        })
        .collect()
}

/// Stack cost of the given *defined* function is the sum of it's locals count (that is,
/// number of arguments plus number of local variables) and the maximal stack
/// height.
fn compute_stack_cost(func_idx: u32, module: &elements::Module) -> Result<u32, Error> {
    // To calculate the cost of a function we need to convert index from
    // function index space to defined function spaces.
    let func_imports = module.import_count(elements::ImportCountType::Function) as u32;
    let defined_func_idx = func_idx
        .checked_sub(func_imports)
        .ok_or_else(|| Error("This should be a index of a defined function".into()))?;

    let code_section = module
        .code_section()
        .ok_or_else(|| Error("Due to validation code section should exists".into()))?;
    let body = &code_section
        .bodies()
        .get(defined_func_idx as usize)
        .ok_or_else(|| Error("Function body is out of bounds".into()))?;
    let locals_count = body.locals().len() as u32;

    let max_stack_height = max_height::compute(defined_func_idx, module)?;

    Ok(locals_count + max_stack_height)
}

fn instrument_functions(ctx: &mut Context, module: &mut elements::Module) -> Result<(), Error> {
    for section in module.sections_mut() {
        if let elements::Section::Code(code_section) = section {
            for func_body in code_section.bodies_mut() {
                let opcodes = func_body.code_mut();
                instrument_function(ctx, opcodes)?;
            }
        }
    }
    Ok(())
}

/// This function searches `call` instructions and wrap each call
/// with preamble and postamble.
///
/// Before:
///
/// ```text
/// get_local 0
/// get_local 1
/// call 228
/// drop
/// ```
///
/// After:
///
/// ```text
/// get_local 0
/// get_local 1
///
/// < ... preamble ... >
///
/// call 228
///
/// < .. postamble ... >
///
/// drop
/// ```
fn instrument_function(
    ctx: &mut Context,
    instructions: &mut elements::Instructions,
) -> Result<(), Error> {
    use parity_wasm::elements::Instruction::*;

    let mut cursor = 0;
    loop {
        if cursor >= instructions.elements().len() {
            break;
        }

        enum Action {
            InstrumentCall {
                callee_idx: u32,
                callee_stack_cost: u32,
            },
            Nop,
        }

        let action: Action = {
            let instruction = &instructions.elements()[cursor];
            match instruction {
                Call(callee_idx) => {
                    let callee_stack_cost = ctx.stack_cost(*callee_idx).ok_or_else(|| {
                        Error(format!(
                            "Call to function that out-of-bounds: {}",
                            callee_idx
                        ))
                    })?;

                    // Instrument only calls to a functions which stack_cost is
                    // non-zero.
                    if callee_stack_cost > 0 {
                        Action::InstrumentCall {
                            callee_idx: *callee_idx,
                            callee_stack_cost,
                        }
                    } else {
                        Action::Nop
                    }
                }
                _ => Action::Nop,
            }
        };

        match action {
            // We need to wrap a `call idx` instruction
            // with a code that adjusts stack height counter
            // and then restores it.
            Action::InstrumentCall {
                callee_idx,
                callee_stack_cost,
            } => {
                let new_seq = instrument_call!(
                    callee_idx,
                    callee_stack_cost as i32,
                    ctx.stack_height_global_idx(),
                    ctx.stack_limit()
                );

                // Replace the original `call idx` instruction with
                // a wrapped call sequence.
                //
                // To splice actually take a place, we need to consume iterator
                // splice returns. So we just `count()` it.
                let _ = instructions
                    .elements_mut()
                    .splice(cursor..(cursor + 1), new_seq.iter().cloned())
                    .count();

                // Advance cursor to be after the inserted sequence.
                cursor += new_seq.len();
            }
            // Do nothing for other instructions.
            _ => {
                cursor += 1;
            }
        }
    }

    Ok(())
}

fn resolve_func_type(
    func_idx: u32,
    module: &elements::Module,
) -> Result<&elements::FunctionType, Error> {
    let types = module.type_section().map(|ts| ts.types()).unwrap_or(&[]);
    let functions = module
        .function_section()
        .map(|fs| fs.entries())
        .unwrap_or(&[]);

    let func_imports = module.import_count(elements::ImportCountType::Function);
    let sig_idx = if func_idx < func_imports as u32 {
        module
            .import_section()
            .expect("function import count is not zero; import section must exists; qed")
            .entries()
            .iter()
            .filter_map(|entry| match entry.external() {
                elements::External::Function(idx) => Some(*idx),
                _ => None,
            })
            .nth(func_idx as usize)
            .expect(
                "func_idx is less than function imports count;
				nth function import must be `Some`;
				qed",
            )
    } else {
        functions
            .get(func_idx as usize - func_imports)
            .ok_or_else(|| Error(format!("Function at index {} is not defined", func_idx)))?
            .type_ref()
    };
    let Type::Function(ty) = types.get(sig_idx as usize).ok_or_else(|| {
        Error(format!(
            "Signature {} (specified by func {}) isn't defined",
            sig_idx, func_idx
        ))
    })?;
    Ok(ty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parity_wasm::elements;

    fn parse_wat(source: &str) -> elements::Module {
        elements::deserialize_buffer(&wabt::wat2wasm(source).expect("Failed to wat2wasm"))
            .expect("Failed to deserialize the module")
    }

    fn validate_module(module: elements::Module) {
        let binary = elements::serialize(module).expect("Failed to serialize");
        wabt::Module::read_binary(binary, &Default::default())
            .expect("Wabt failed to read final binary")
            .validate()
            .expect("Invalid module");
    }

    #[test]
    fn test_with_params_and_result() {
        let module = parse_wat(
            r#"
(module
	(func (export "i32.add") (param i32 i32) (result i32)
		get_local 0
	get_local 1
	i32.add
	)
)
"#,
        );

        let module = inject_limiter(module, 1024).expect("Failed to inject stack counter");
        validate_module(module);
    }
}
