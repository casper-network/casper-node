#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

#[macro_use]
extern crate alloc;

use alloc::{string::String, vec::Vec};
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

            let call1 = "set_greeting";
            let input_data1: (String,) = ("Foo".into(),);
            let res1 = host::call(
                &contract_address,
                0,
                call1,
                &borsh::to_vec(&input_data1).unwrap(),
            );

            log!("{call1:?} result={res1:?}");

            let call2 = "get_greeting";
            let res2 = host::call(&contract_address, 0, call2, &[]);

            log!("{call2:?} result={res2:?}")
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
