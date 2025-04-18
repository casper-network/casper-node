use wasmer::{wasmparser::Operator, FunctionMiddleware, MiddlewareError, ModuleMiddleware};

const MIDDLEWARE_NAME: &str = "Gatekeeper";
const FLOATING_POINTS_NOT_ALLOWED: &str = "Floating point opcodes are not allowed";

#[inline]
fn extension_not_allowed_error(extension: &str) -> MiddlewareError {
    MiddlewareError::new(
        MIDDLEWARE_NAME,
        format!("Wasm `{extension}` extension is not allowed"),
    )
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GatekeeperConfig {
    /// Allow the `bulk_memory` proposal.
    bulk_memory: bool,
    /// Allow the `exceptions` proposal.
    exceptions: bool,
    /// Allow the `function_references` proposal.
    function_references: bool,
    /// Allow the `gc` proposal.
    gc: bool,
    /// Allow the `legacy_exceptions` proposal.
    #[allow(dead_code)]
    legacy_exceptions: bool,
    /// Allow the `memory_control` proposal.
    memory_control: bool,
    /// Allow the `mvp` proposal.
    mvp: bool,
    /// Allow the `reference_types` proposal.
    reference_types: bool,
    /// Allow the `relaxed_simd` proposal.
    relaxed_simd: bool,
    /// Allow the `saturating_float_to_int` proposal.
    ///
    /// This *requires* canonicalized NaNs enabled in the compiler config.
    saturating_float_to_int: bool,
    /// Allow the `shared_everything_threads` proposal.
    #[allow(dead_code)]
    shared_everything_threads: bool,
    /// Allow the `sign_extension` proposal.
    sign_extension: bool,
    /// Allow the `simd` proposal.
    simd: bool,
    /// Allow the `stack_switching` proposal.
    #[allow(dead_code)]
    stack_switching: bool,
    /// Allow the `tail_call` proposal.
    tail_call: bool,
    /// Allow the `threads` proposal.
    threads: bool,
    /// Allow the `wide_arithmetic` proposal.
    #[allow(dead_code)]
    wide_arithmetic: bool,
    /// Allow floating point opcodes from `mvp` extension.
    ///
    /// This *requires* canonicalized NaNs enabled in the compiler config.
    allow_floating_points: bool,
}

/// Check if the operator is a floating point operator.
#[inline]
const fn is_floating_point(operator: &wasmer::wasmparser::Operator<'_>) -> bool {
    match operator {
        // mvp
        Operator::F32Load {..} |
        Operator::F64Load {..} |
        Operator::F32Store {..} |
        Operator::F64Store {..} |
        Operator::F32Const {..} |
        Operator::F64Const {..} |
        Operator::F32Abs |
        Operator::F32Neg |
        Operator::F32Ceil |
        Operator::F32Floor |
        Operator::F32Trunc |
        Operator::F32Nearest |
        Operator::F32Sqrt |
        Operator::F32Add |
        Operator::F32Sub |
        Operator::F32Mul |
        Operator::F32Div |
        Operator::F32Min |
        Operator::F32Max |
        Operator::F32Copysign |
        Operator::F64Abs |
        Operator::F64Neg |
        Operator::F64Ceil |
        Operator::F64Floor |
        Operator::F64Trunc |
        Operator::F64Nearest |
        Operator::F64Sqrt |
        Operator::F64Add |
        Operator::F64Sub |
        Operator::F64Mul |
        Operator::F64Div |
        Operator::F64Min |
        Operator::F64Max |
        Operator::F64Copysign |
        Operator::F32Eq |
        Operator::F32Ne |
        Operator::F32Lt |
        Operator::F32Gt |
        Operator::F32Le |
        Operator::F32Ge |
        Operator::F64Eq |
        Operator::F64Ne |
        Operator::F64Lt |
        Operator::F64Gt |
        Operator::F64Le |
        Operator::F64Ge |
        Operator::I32TruncF32S |
        Operator::I32TruncF32U |
        Operator::I32TruncF64S |
        Operator::I32TruncF64U |
        Operator::I64TruncF32S |
        Operator::I64TruncF32U |
        Operator::I64TruncF64S |
        Operator::I64TruncF64U |
        Operator::F32ConvertI32S |
        Operator::F32ConvertI32U |
        Operator::F32ConvertI64S |
        Operator::F32ConvertI64U |
        Operator::F32DemoteF64 |
        Operator::F64ConvertI32S |
        Operator::F64ConvertI32U |
        Operator::F64ConvertI64S |
        Operator::F64ConvertI64U |
        Operator::F64PromoteF32 |
        Operator::I32ReinterpretF32 |
        Operator::I64ReinterpretF64 |
        Operator::F32ReinterpretI32 |
        Operator::F64ReinterpretI64 |
        // saturating_float_to_int
        Operator::I32TruncSatF32S |
        Operator::I32TruncSatF32U |
        Operator::I32TruncSatF64S |
        Operator::I32TruncSatF64U |
        Operator::I64TruncSatF32S |
        Operator::I64TruncSatF32U |
        Operator::I64TruncSatF64S |
        Operator::I64TruncSatF64U |
        // simd
        Operator::F32x4ExtractLane{..} |
        Operator::F32x4ReplaceLane{..} |
        Operator::F64x2ExtractLane{..} |
        Operator::F64x2ReplaceLane{..} |
        Operator::F32x4Splat |
        Operator::F64x2Splat |
        Operator::F32x4Eq |
        Operator::F32x4Ne |
        Operator::F32x4Lt |
        Operator::F32x4Gt |
        Operator::F32x4Le |
        Operator::F32x4Ge |
        Operator::F64x2Eq |
        Operator::F64x2Ne |
        Operator::F64x2Lt |
        Operator::F64x2Gt |
        Operator::F64x2Le |
        Operator::F64x2Ge |
        Operator::F32x4Ceil |
        Operator::F32x4Floor |
        Operator::F32x4Trunc |
        Operator::F32x4Nearest |
        Operator::F32x4Abs |
        Operator::F32x4Neg |
        Operator::F32x4Sqrt |
        Operator::F32x4Add |
        Operator::F32x4Sub |
        Operator::F32x4Mul |
        Operator::F32x4Div |
        Operator::F32x4Min |
        Operator::F32x4Max |
        Operator::F32x4PMin |
        Operator::F32x4PMax |
        Operator::F64x2Ceil |
        Operator::F64x2Floor |
        Operator::F64x2Trunc |
        Operator::F64x2Nearest |
        Operator::F64x2Abs |
        Operator::F64x2Neg |
        Operator::F64x2Sqrt |
        Operator::F64x2Add |
        Operator::F64x2Sub |
        Operator::F64x2Mul |
        Operator::F64x2Div |
        Operator::F64x2Min |
        Operator::F64x2Max |
        Operator::F64x2PMin |
        Operator::F64x2PMax |
        Operator::I32x4TruncSatF32x4S |
        Operator::I32x4TruncSatF32x4U |
        Operator::F32x4ConvertI32x4S |
        Operator::F32x4ConvertI32x4U |
        Operator::I32x4TruncSatF64x2SZero |
        Operator::I32x4TruncSatF64x2UZero |
        Operator::F64x2ConvertLowI32x4S |
        Operator::F64x2ConvertLowI32x4U |
        Operator::F32x4DemoteF64x2Zero |
        Operator::F64x2PromoteLowF32x4 |
        // relaxed_simd extension
        Operator::I32x4RelaxedTruncF32x4S |
        Operator::I32x4RelaxedTruncF32x4U |
        Operator::I32x4RelaxedTruncF64x2SZero |
        Operator::I32x4RelaxedTruncF64x2UZero |
        Operator::F32x4RelaxedMadd |
        Operator::F32x4RelaxedNmadd |
        Operator::F64x2RelaxedMadd |
        Operator::F64x2RelaxedNmadd |
        Operator::F32x4RelaxedMin |
        Operator::F32x4RelaxedMax |
        Operator::F64x2RelaxedMin |
        Operator::F64x2RelaxedMax => true,
        _ => false,
    }
}

impl Default for GatekeeperConfig {
    fn default() -> Self {
        Self {
            bulk_memory: false,
            exceptions: false,
            function_references: false,
            gc: false,
            legacy_exceptions: false,
            memory_control: false,
            mvp: true,
            reference_types: false,
            relaxed_simd: false,
            saturating_float_to_int: false,
            shared_everything_threads: false,
            sign_extension: true,
            simd: false,
            stack_switching: false,
            tail_call: false,
            threads: false,
            wide_arithmetic: false,
            // Not yet ready to enable this; needs updated benchmark to accomodate overhead of
            // canonicalized NaNs and manual validation.
            allow_floating_points: false,
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct Gatekeeper {
    config: GatekeeperConfig,
}

impl Gatekeeper {
    pub(crate) fn new(config: GatekeeperConfig) -> Self {
        Self { config }
    }
}

impl ModuleMiddleware for Gatekeeper {
    fn generate_function_middleware(
        &self,
        _local_function_index: wasmer::LocalFunctionIndex,
    ) -> Box<dyn wasmer::FunctionMiddleware> {
        Box::new(FunctionGatekeeper::new(self.config))
    }
}

#[derive(Debug)]
struct FunctionGatekeeper {
    config: GatekeeperConfig,
}

impl FunctionGatekeeper {
    fn new(config: GatekeeperConfig) -> Self {
        Self { config }
    }

    /// Ensure that floating point opcodes are allowed.
    fn ensure_floating_point_allowed(
        &self,
        operator: &wasmer::wasmparser::Operator<'_>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if !self.config.allow_floating_points && is_floating_point(operator) {
            return Err(MiddlewareError::new(
                MIDDLEWARE_NAME,
                FLOATING_POINTS_NOT_ALLOWED,
            ));
        }
        Ok(())
    }

    fn validated_push_operator<'b, 'a: 'b>(
        &self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        // This is a late check as we first check if given extension is allowed and then check if
        // floating point opcodes are allowed. This is because different Wasm extensions do
        // contain floating point opcodes and this approach makes all the gatekeeping more robust.
        self.ensure_floating_point_allowed(&operator)?;
        // Push the operator to the state.
        state.push_operator(operator);
        Ok(())
    }

    fn bulk_memory<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.bulk_memory {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("bulk_memory"))
        }
    }

    fn exceptions<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.exceptions {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("exceptions"))
        }
    }

    fn function_references<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.function_references {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("function_references"))
        }
    }

    fn gc<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.gc {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("gc"))
        }
    }

    #[allow(dead_code)]
    fn legacy_exceptions<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.legacy_exceptions {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("legacy_exceptions"))
        }
    }

    fn memory_control<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.memory_control {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("memory_control"))
        }
    }

    fn mvp<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.mvp {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("mvp"))
        }
    }

    fn reference_types<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.reference_types {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("reference_types"))
        }
    }

    fn relaxed_simd<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.relaxed_simd {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("relaxed_simd"))
        }
    }

    fn saturating_float_to_int<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.saturating_float_to_int {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("saturating_float_to_int"))
        }
    }

    #[allow(dead_code)]
    fn shared_everything_threads<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.shared_everything_threads {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("shared_everything_threads"))
        }
    }

    fn sign_extension<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.sign_extension {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("sign_extension"))
        }
    }

    fn simd<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.simd {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("simd"))
        }
    }

    #[allow(dead_code)]
    fn stack_switching<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.stack_switching {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("stack_switching"))
        }
    }

    fn tail_call<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.tail_call {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("tail_call"))
        }
    }
    fn threads<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.threads {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("threads"))
        }
    }

    #[allow(dead_code)]
    fn wide_arithmetic<'b, 'a: 'b>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'b>,
    ) -> Result<(), wasmer::MiddlewareError> {
        if self.config.wide_arithmetic {
            self.validated_push_operator(operator, state)?;
            Ok(())
        } else {
            Err(extension_not_allowed_error("wide_arithmetic"))
        }
    }
}

impl FunctionMiddleware for FunctionGatekeeper {
    fn feed<'a>(
        &mut self,
        operator: wasmer::wasmparser::Operator<'a>,
        state: &mut wasmer::MiddlewareReaderState<'a>,
    ) -> Result<(), wasmer::MiddlewareError> {
        macro_rules! match_op {
            ($op:ident { $($payload:tt)* }) => {
                $op { .. }
            };
            ($op:ident) => {
                $op
            };
        }

        macro_rules! gatekeep {
          ($( @$proposal:ident $op:ident $({ $($payload:tt)* })? => $visit:ident)*) => {{
                use wasmer::wasmparser::Operator::*;
                match operator {
                    $(
                        match_op!($op $({ $($payload)* })?) => self.$proposal(operator, state),
                    )*
                }
            }}
        }

        wasmer::wasmparser::for_each_operator!(gatekeep)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use wasmer::{sys::EngineBuilder, CompilerConfig, Module, Singlepass, Store, WasmError};

    #[test]
    fn mvp_opcodes_allowed() {
        let bytecode = wat::parse_str(
            r#"
            (module
                (func (export "add") (param i32 i32) (result i32)
                    local.get 0
                    local.get 1
                    i32.add)

            )
            "#,
        )
        .unwrap();
        let mut gatekeeper = Gatekeeper::default();
        gatekeeper.config.mvp = true;
        let gatekeeper = Arc::new(gatekeeper);
        let mut compiler_config = Singlepass::default();
        compiler_config.push_middleware(gatekeeper);
        let store = Store::new(EngineBuilder::new(compiler_config));
        let _module = Module::new(&store, &bytecode).unwrap();
    }
    #[test]
    fn mvp_opcodes_allowed_without_floating_points() {
        let bytecode = wat::parse_str(
            r#"
            (module
                (func (export "add") (param f32 f32) (result f32)
                    local.get 0
                    local.get 1
                    f32.add)
            )
            "#,
        )
        .unwrap();
        let mut gatekeeper = Gatekeeper::default();
        gatekeeper.config.mvp = true;
        gatekeeper.config.allow_floating_points = false;
        let gatekeeper = Arc::new(gatekeeper);
        let mut compiler_config = Singlepass::default();
        compiler_config.push_middleware(gatekeeper);
        let store = Store::new(EngineBuilder::new(compiler_config));
        let error = Module::new(&store, &bytecode).unwrap_err();
        let middleware = match error {
            wasmer::CompileError::Wasm(WasmError::Middleware(middleware)) => middleware,
            _ => panic!("Expected a middleware error"),
        };
        assert_eq!(middleware.message, FLOATING_POINTS_NOT_ALLOWED);
    }

    #[test]
    fn mvp_opcodes_allowed_with_floating_points() {
        let bytecode = wat::parse_str(
            r#"
            (module
                (func (export "add") (param f32 f32) (result f32)
                    local.get 0
                    local.get 1
                    f32.add)
            )
            "#,
        )
        .unwrap();
        let mut gatekeeper = Gatekeeper::default();
        gatekeeper.config.mvp = true;
        gatekeeper.config.allow_floating_points = true;
        let gatekeeper = Arc::new(gatekeeper);
        let mut compiler_config = Singlepass::default();
        compiler_config.push_middleware(gatekeeper);
        let store = Store::new(EngineBuilder::new(compiler_config));
        let _module = Module::new(&store, &bytecode).unwrap();
    }
    #[test]
    fn mvp_opcodes_not_allowed() {
        let bytecode = wat::parse_str(
            r#"
            (module
                (func (export "add") (param i32 i32) (result i32)
                    local.get 0
                    local.get 1
                    i32.add)
            )
            "#,
        )
        .unwrap();
        let mut gatekeeper = Gatekeeper::default();
        gatekeeper.config.mvp = false;
        let gatekeeper = Arc::new(gatekeeper);
        let mut compiler_config = Singlepass::default();
        compiler_config.push_middleware(gatekeeper);
        let store = Store::new(EngineBuilder::new(compiler_config));
        let error = Module::new(&store, &bytecode).unwrap_err();
        assert_eq!(error.to_string(), "WebAssembly translation error: Error in middleware Gatekeeper: Wasm `mvp` extension is not allowed");
    }
}
