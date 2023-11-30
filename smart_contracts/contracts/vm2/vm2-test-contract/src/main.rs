#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

#[macro_use]
extern crate alloc;

use alloc::{string::String, vec::Vec};
use casper_macros::{casper, Contract};
use casper_sdk::{log, Value};

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

mod exports {

    use alloc::{string::String, vec::Vec};
    use borsh::BorshSerialize;
    use casper_macros::casper;
    use casper_sdk::{
        host::{self, CreateResult, EntryPoint, Manifest, Param},
        reserve_vec_space,
    };
    use core::ptr::NonNull;

    // use crate::reserve_vec_space;

    const KEY_SPACE_DEFAULT: u64 = 0;
    const TAG_BYTES: u64 = 0;

    #[casper(export)]
    pub fn call(arg1: String, arg2: u32) {
        // (String, u32) = deserialize

        // (arg1, arg2): (String, u32) = Deserialize(input);
        host::casper_print(&format!("arg1: {arg1}"));
        host::casper_print(&format!("arg2: {arg2}"));

        let mut read1 = Vec::new();

        let _non_existing_entry = host::casper_read(KEY_SPACE_DEFAULT, b"hello", |size| {
            host::casper_print(&format!("first cb alloc cb with size={size}"));
            reserve_vec_space(&mut read1, size)
            // static_buffer.as_mut_ptr()
        })
        .expect("should read");
        // host::casper_print(&format!("non_existing_entry={:?}", non_existing_entry));

        host::casper_write(KEY_SPACE_DEFAULT, b"hello", TAG_BYTES, b"Hello, world!").unwrap();

        let mut read2 = Vec::new();
        let existing_entry = host::casper_read(KEY_SPACE_DEFAULT, b"hello", |size| {
            host::casper_print(&format!("second cb alloc cb with size={size}"));
            reserve_vec_space(&mut read2, size)
        })
        .expect("should read")
        .expect("should have entry");
        host::casper_print(&format!("existing_entry={:?}", existing_entry));
        let msg = String::from_utf8(read2).unwrap();
        host::casper_print(&format!("existing_entry={:?}", msg));

        host::casper_write(KEY_SPACE_DEFAULT, b"read back", TAG_BYTES, msg.as_bytes()).unwrap();

        const PARAM_1: &str = "param_1";
        const PARAM_2: &str = "param_2";

        let _params = [
            Param {
                name_ptr: PARAM_1.as_ptr(),
                name_len: PARAM_1.len(),
                ty: 0,
            },
            Param {
                name_ptr: PARAM_2.as_ptr(),
                name_len: PARAM_2.len(),
                ty: 0,
            },
        ];

        extern "C" fn mangled_entry_point_wrapper_1() {
            host::start(|(name, value): (String, u32)| {
                host::casper_print(&format!("Hello, world! Name={:?} value={:?}", name, value));
            })
            // let input_bytes = host::casper_copy_input();
            // // mangled_entry_point(param_1, param_2)
            // host::casper_print("called inside a mangled entry point 1");
            // // host::casper_print()
        }

        extern "C" fn mangled_entry_point_wrapper_2() {
            // mangled_entry_point(param_1, param_2)
            host::casper_print("called inside a mangled entry point 2");
        }

        let name_1 = "mangled_entry_point_1";
        let name_2 = "mangled_entry_point_2";

        let param_1 = "param_1";
        let param_2 = "param_2";

        let params = [
            Param {
                name_ptr: param_1.as_ptr(),
                name_len: param_1.len(),
                ty: 1,
            },
            Param {
                name_ptr: param_2.as_ptr(),
                name_len: param_2.len(),
                ty: 2,
            },
        ];

        type _Foo = extern "C" fn() -> ();

        // host::casper_print(&format!("{:?}", mem::size_of::<
        let entry_point_1 = EntryPoint {
            name_ptr: name_1.as_ptr(),
            name_len: name_1.len(),
            params_ptr: NonNull::from(params.as_slice()).as_ptr() as _,
            params_size: params.len(),
            fptr: mangled_entry_point_wrapper_1,
        };

        let entry_point_2 = EntryPoint {
            name_ptr: name_2.as_ptr(),
            name_len: name_2.len(),
            params_ptr: NonNull::from(params.as_slice()).as_ptr() as _,
            params_size: params.len(),
            fptr: mangled_entry_point_wrapper_2,
        };
        let entry_points = [entry_point_1, entry_point_2];

        // struct Foo{};

        host::casper_print(&format!(
            "Foo size/align {}/{}",
            core::mem::size_of::<*const [Param]>(),
            core::mem::size_of::<*const Param>()
        ));

        host::casper_print(&format!(
            "{:?} (sz={})",
            &entry_points,
            core::mem::size_of::<EntryPoint>()
        ));

        let manifest = Manifest {
            entry_points: entry_points.as_ptr(),
            entry_points_size: entry_points.len(),
        };
        host::casper_print(&format!("manifest {:?}", manifest));
        let res = host::casper_create(None, &manifest);
        host::casper_print(&format!("create res {:?}", res));

        match res {
            Ok(CreateResult {
                package_address,
                contract_address,
                version,
            }) => {
                host::casper_print("success");

                let input_data = borsh::to_vec(&("Hello, world!", 42)).unwrap();

                let call_result =
                    host::casper_call(&contract_address, 10, "mangled_entry_point_1", &input_data);
                match call_result {
                    Ok(output_data) => {
                        host::casper_print(&format!(
                            "✅ Call succeeded. Output: {:?}",
                            output_data
                        ));
                    }
                    Err(error) => {
                        host::casper_print(&format!("❌ Call failed. Error: {:?}", error));
                    }
                }
            }
            Err(error) => {
                host::casper_print(&format!("error {:?}", error));
            }
        }

        host::casper_print("👋 Goodbye");
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    todo!()
}

#[casper(export)]
pub fn foobar() {}

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
        // dbg!(schema_helpe)
    }

    #[test]
    fn compile_time_schema() {
        let schema = Greeter::schema();
        // dbg!(&schema);
        assert_eq!(schema.name, "Greeter");
        assert_eq!(schema.entry_points[0].name, "get_greeting");
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
