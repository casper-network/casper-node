use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use casper_types::WasmConfig;

use casper_execution_engine::runtime::{self, PreprocessingError};
use casper_wasmi::{
    memory_units::Pages, Error, Externals, FuncInstance, HostError, ImportsBuilder, MemoryInstance,
    ModuleImportResolver, ModuleInstance, RuntimeValue, Signature,
};

#[derive(Debug, thiserror::Error)]
enum WasmPreparationError {
    #[error("Error when preprocessing {0}")]
    Preprocessing(PreprocessingError),
    #[error("Wasmi error {0}")]
    Wasmi(Error),
    #[error("Unable to export by name {0}")]
    UnableToExportByName(String),
    #[error("Unable to convert to function {0}")]
    UnableToFunc(String),
    #[error("Type casting error")]
    TypeCasting,
}

impl From<PreprocessingError> for WasmPreparationError {
    fn from(value: PreprocessingError) -> Self {
        WasmPreparationError::Preprocessing(value)
    }
}

impl From<Error> for WasmPreparationError {
    fn from(value: Error) -> Self {
        WasmPreparationError::Wasmi(value)
    }
}

fn prepare_instance(
    module_bytes: &[u8],
    chainspec: &ChainspecConfig,
) -> Result<casper_wasmi::ModuleRef, WasmPreparationError> {
    let wasm_module = runtime::preprocess(chainspec.wasm_config, module_bytes)?;
    let module = casper_wasmi::Module::from_casper_wasm_module(wasm_module)?;
    let resolver = MinimalWasmiResolver::default();
    let mut imports = ImportsBuilder::new();
    imports.push_resolver("env", &resolver);
    let not_started_module = ModuleInstance::new(&module, &imports)?;

    assert!(!not_started_module.has_start());

    let instance = not_started_module.not_started_instance();
    Ok(instance.clone())
}

struct RunWasmInfo {
    elapsed: Duration,
    gas_used: u64,
}

fn run_wasm(
    module_bytes: Vec<u8>,
    cli_args: &Args,
    chainspec: &ChainspecConfig,
    func_name: &str,
) -> Result<(Result<Option<RuntimeValue>, Error>, RunWasmInfo), WasmPreparationError> {
    println!(
        "Invoke export {:?} with args {:?}",
        func_name, cli_args.args
    );

    let instance = prepare_instance(&module_bytes, chainspec)?;

    let params = {
        let export = instance.export_by_name(func_name).ok_or(
            WasmPreparationError::UnableToExportByName(func_name.to_owned()),
        )?;
        let func = export
            .as_func()
            .ok_or(WasmPreparationError::UnableToFunc(func_name.to_owned()))?;
        func.signature().params().to_owned()
    };

    let args = {
        assert_eq!(
            cli_args.args.len(),
            params.len(),
            "Not enough arguments supplied"
        );
        let mut vec = Vec::new();
        for (input_arg, func_arg) in cli_args.args.iter().zip(params.into_iter()) {
            let value = match func_arg {
                casper_wasmi::ValueType::I32 => casper_wasmi::RuntimeValue::I32(
                    input_arg
                        .parse()
                        .map_err(|_| WasmPreparationError::TypeCasting)?,
                ),
                casper_wasmi::ValueType::I64 => casper_wasmi::RuntimeValue::I64(
                    input_arg
                        .parse()
                        .map_err(|_| WasmPreparationError::TypeCasting)?,
                ),
                casper_wasmi::ValueType::F32 => todo!(),
                casper_wasmi::ValueType::F64 => todo!(),
            };
            vec.push(value);
        }
        vec
    };

    let start = Instant::now();

    let gas_limit = cli_args
        .gas_limit
        .unwrap_or(chainspec.transaction_config.block_gas_limit);

    let mut externals = MinimalWasmiExternals::new(0, gas_limit);
    let result: Result<Option<RuntimeValue>, Error> =
        instance
            .clone()
            .invoke_export(func_name, &args, &mut externals);

    let info = RunWasmInfo {
        elapsed: start.elapsed(),
        gas_used: externals.gas_used,
    };

    Ok((result, info))
}
use clap::Parser;
use serde::Deserialize;

#[derive(Parser, Clone, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(value_name = "MODULE")]
    wasm_file: PathBuf,
    #[arg(long = "gas_limit")]
    gas_limit: Option<u64>,
    #[arg(long = "invoke", value_name = "FUNCTION")]
    invoke: Option<String>,
    /// Arguments given to the Wasm module or the invoked function.
    #[arg(value_name = "ARGS")]
    args: Vec<String>,
    #[arg(short, long)]
    chainspec_file: Option<PathBuf>,
}

fn load_wasm_file<P: AsRef<Path>>(path: P) -> Result<Vec<u8>, String> {
    let path = path.as_ref();
    let bytes =
        fs::read(path).map_err(|err| format!("failed to read file {path:?}. Reason {err:?}"))?;
    match path.extension() {
        Some(ext) if ext.eq_ignore_ascii_case("wat") => wat::parse_bytes(&bytes)
            .map(|el| el.into_owned())
            .map_err(|err| format!("Expected valid wat, got error: {err}")),
        None | Some(_) => Ok(bytes),
    }
}

#[derive(Deserialize, Clone, Default, Debug)]
struct TransactionConfig {
    block_gas_limit: u64,
}

/// in the chainspec file, it can continue to be parsed as an `ChainspecConfig`.
#[derive(Deserialize, Clone, Default, Debug)]
struct ChainspecConfig {
    /// WasmConfig.
    #[serde(rename = "wasm")]
    pub wasm_config: WasmConfig,
    #[serde(rename = "transactions")]
    pub transaction_config: TransactionConfig,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    let chainspec_file = args
        .clone()
        .chainspec_file
        .ok_or("Expected chainspec file to be present")?;
    println!("Using chainspec file {:?}", chainspec_file.display());
    let chainspec_data = fs::read_to_string(chainspec_file.as_path())
        .map_err(|err| format!("Error when reading chainspec file: {err}"))?;
    let chainspec_config: ChainspecConfig = toml::from_str(&chainspec_data)
        .map_err(|err| format!("Error when parsing chainspec file: {err}"))?;

    let wasm_bytes = load_wasm_file(&args.wasm_file)
        .map_err(|err| format!("Error when loading wasm file: {err}"))?;

    if let Some(ref func_name) = args.invoke {
        let (result, info) = run_wasm(wasm_bytes, &args, &chainspec_config, func_name)?;

        println!("result: {:?}", result);
        println!("elapsed: {:?}", info.elapsed);
        println!("gas used: {}", info.gas_used);
    }
    Ok(())
}

#[derive(Default)]
struct MinimalWasmiResolver(());

#[derive(Debug)]
struct MinimalWasmiExternals {
    gas_used: u64,
    block_gas_limit: u64,
}

impl MinimalWasmiExternals {
    fn new(gas_used: u64, block_gas_limit: u64) -> Self {
        Self {
            gas_used,
            block_gas_limit,
        }
    }
}

const GAS_FUNC_IDX: usize = 0;

impl ModuleImportResolver for MinimalWasmiResolver {
    fn resolve_func(
        &self,
        field_name: &str,
        _signature: &casper_wasmi::Signature,
    ) -> Result<casper_wasmi::FuncRef, Error> {
        if field_name == "gas" {
            Ok(FuncInstance::alloc_host(
                Signature::new(&[casper_wasmi::ValueType::I32; 1][..], None),
                GAS_FUNC_IDX,
            ))
        } else {
            Err(Error::Instantiation(format!(
                "Export {} not found",
                field_name
            )))
        }
    }

    fn resolve_memory(
        &self,
        field_name: &str,
        memory_type: &casper_wasmi::MemoryDescriptor,
    ) -> Result<casper_wasmi::MemoryRef, Error> {
        if field_name == "memory" {
            Ok(MemoryInstance::alloc(
                Pages(memory_type.initial() as usize),
                memory_type.maximum().map(|x| Pages(x as usize)),
            )?)
        } else {
            panic!("invalid exported memory name {}", field_name);
        }
    }
}

#[derive(thiserror::Error, Debug)]
#[error("gas limit")]
struct GasLimit;

impl HostError for GasLimit {}

impl Externals for MinimalWasmiExternals {
    fn invoke_index(
        &mut self,
        index: usize,
        args: casper_wasmi::RuntimeArgs,
    ) -> Result<Option<casper_wasmi::RuntimeValue>, casper_wasmi::Trap> {
        if index == GAS_FUNC_IDX {
            let gas_used: u32 = args.nth_checked(0)?;
            // match gas_used.checked_add(
            match self.gas_used.checked_add(gas_used.into()) {
                Some(new_gas_used) if new_gas_used > self.block_gas_limit => {
                    return Err(GasLimit.into());
                }
                Some(new_gas_used) => {
                    // dbg!(&new_gas_used, &self.block_gas_limit);
                    self.gas_used = new_gas_used;
                }
                None => {
                    unreachable!();
                }
            }
            Ok(None)
        } else {
            unreachable!();
        }
    }
}
