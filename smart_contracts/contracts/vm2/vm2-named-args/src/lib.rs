#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

use casper_contract_sdk::{
    compat::types::{CLType, CLValue, RuntimeArgs},
    prelude::*,
};

/// This contract implements a simple flipper.
#[derive(PanicOnDefault)]
#[casper(contract_state)]
pub struct Contract;

#[casper]
impl Contract {
    #[casper(ignore_state)]
    pub fn accepts_named_args_1(runtime_args: RuntimeArgs) -> CLValue {
        // Simpliest example of a contract that accepts named arguments.
        // It retrieves the named argument "to" and returns a greeting message.
        // RuntimeArgs is deserialized from the input data, and a CLValue is returned as serialized
        // bytes.

        // Retrieve the named argument "flipped"
        let to: String = runtime_args
            .get("to")
            .and_then(|arg| arg.to_t().ok())
            .expect("Named argument 'to' not found or has wrong type");

        let result = format!("Hello, {to}!");

        CLValue::from_t(result).unwrap()
    }

    #[casper(manual)]
    pub fn uses_compatibility_layer(&self) {
        // This variant does not use the compatibility layer, but still has a state.
    }

    #[casper(ignore_state, manual)]
    pub fn uses_compatibility_layer_no_state() {
        // No self parameter so no state, and uses compatibility layer for args.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flipper() {
        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert("to", String::from("world")).unwrap();

        let result = Contract::accepts_named_args_1(runtime_args);

        let result: String = result.into_t().expect("Failed to convert result to String");
        assert_eq!(result, "Hello, world!");
    }
}
