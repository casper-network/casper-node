#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

#[macro_use]
extern crate alloc;

use alloc::{
    string::{String, ToString},
    vec::Vec,
};
use casper_macros::{casper, Contract};
use casper_sdk::{
    host::{self, CreateResult},
    log, Value,
};

#[derive(Contract)]
struct Greeter {
    greeting: Value<String>,
}

#[casper(entry_points)]
impl Greeter {
    pub fn get_greeting(&self) -> String {
        self.greeting
            .read()
            .expect("should read value")
            .unwrap_or_default()
    }
    pub fn set_greeting(&mut self, greeting: String) {
        log!("Saving greeting {}", greeting);
        self.greeting.write(greeting).unwrap();
    }

    pub fn emit_unreachable_trap(&self) -> ! {
        #[cfg(target_arch = "wasm32")]
        {
            unsafe { core::arch::wasm32::unreachable() }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            panic!("unreachable")
        }
    }
}

use casper_sdk::Contract;

#[casper(export)]
pub fn call() {
    log!("calling create");
    match Greeter::create() {
        Ok(CreateResult {
            package_address,
            contract_address,
            version,
        }) => {
            log!("success");
            log!("package_address: {:?}", package_address);
            log!("contract_address: {:?}", contract_address);
            log!("version: {:?}", version);

            let call0 = "get_greeting";
            let res0 = host::casper_call(&contract_address, 0, call0, &[]).unwrap();
            assert_eq!(borsh::from_slice::<String>(&res0).unwrap(), "".to_string()); // TODO: Constructors
            log!("{call0:?} result={res0:?}");

            let call1 = "set_greeting";
            let input_data1: (String,) = ("Foo".into(),);
            let res1 = host::casper_call(
                &contract_address,
                0,
                call1,
                &borsh::to_vec(&input_data1).unwrap(),
            );

            log!("{call1:?} result={res1:?}");

            let call2 = "get_greeting";
            let res2 = host::casper_call(&contract_address, 0, call2, &[]).unwrap();
            assert_eq!(
                borsh::from_slice::<String>(&res2).unwrap(),
                "Foo".to_string()
            );
            log!("{call2:?} result={res2:?}");

            let call3 = "emit_unreachable_trap";
            let res3 = host::casper_call(&contract_address, 0, call3, &[]).unwrap_err();
            assert_eq!(res3, host::CallError::CalleeTrapped);

            // let call4: &str = "unreachable";
            // let res4 = host::casper_call(&contract_address, 0, call4, &[]);

            // log!("{call4:?} result={res4:?}");
        }
        Err(error) => {
            log!("error {:?}", error);
        }
    }

    log!("👋 Goodbye");
}

#[cfg(test)]
mod tests {

    use casper_sdk::{schema_helper, Contract};

    use super::*;

    #[test]
    fn test() {
        let args = ("hello".to_string(), 123);
        schema_helper::dispatch("call", &borsh::to_vec(&args).unwrap());
    }

    #[test]
    fn exports() {
        dbg!(schema_helper::list_exports());
    }

    #[test]
    fn compile_time_schema() {
        let schema = Greeter::schema();
        dbg!(&schema);
        assert_eq!(schema.name, "Greeter");
        assert_eq!(schema.entry_points[0].name, "get_greeting");
        // assert_eq!(schema.entry_points[0].name, "flip");
        assert_eq!(schema.entry_points[1].name, "set_greeting");
        // let s = serde_json::to_string_pretty(&schema).expect("foo");
        // println!("{s}");
    }

    #[test]
    fn should_greet() {
        assert_eq!(Greeter::name(), "Greeter");
        let mut flipper = Greeter::new();
        assert_eq!(flipper.get_greeting(), ""); // TODO: Initializer
        flipper.set_greeting("Hi".into());
        assert_eq!(flipper.get_greeting(), "Hi");
    }
}
