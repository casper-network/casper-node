#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

#[macro_use]
extern crate alloc;

use casper_macros::{casper, Contract};
use casper_sdk::log;

mod exports {

    use alloc::{string::String, vec::Vec};
    use casper_macros::{casper, selector};

    use casper_sdk::{
        host, reserve_vec_space,
        sys::{CreateResult, EntryPoint, Manifest, Param},
    };
    use core::ptr::NonNull;
    use vm_common::{flags::EntryPointFlags, keyspace::Keyspace, selector::Selector};

    // use crate::reserve_vec_space;

    const KEY_SPACE_DEFAULT: u64 = 0;
    const TAG_BYTES: u64 = 0;

    const MANGLED_ENTRY_POINT_1: Selector = selector!("mangled_entry_point_1");
    const MANGLED_ENTRY_POINT_2: Selector = selector!("mangled_entry_point_2");

    #[casper(export)]
    pub fn call(arg1: String, arg2: u32) {
        // (String, u32) = deserialize

        // (arg1, arg2): (String, u32) = Deserialize(input);
        host::casper_print(&format!("arg1: {arg1}"));
        host::casper_print(&format!("arg2: {arg2}"));

        let mut read1 = Vec::new();

        let _non_existing_entry = host::casper_read(Keyspace::Context(b"hello"), |size| {
            host::casper_print(&format!("first cb alloc cb with size={size}"));
            reserve_vec_space(&mut read1, size)
        })
        .expect("should read");
        // host::casper_print(&format!("non_existing_entry={:?}", non_existing_entry));

        host::casper_write(Keyspace::Context(b"hello"), b"Hello, world!").unwrap();

        let mut read2 = Vec::new();
        let existing_entry = host::casper_read(Keyspace::Context(b"hello"), |size| {
            host::casper_print(&format!("second cb alloc cb with size={size}"));
            reserve_vec_space(&mut read2, size)
        })
        .expect("should read")
        .expect("should have entry");
        host::casper_print(&format!("existing_entry={:?}", existing_entry));
        let msg = String::from_utf8(read2).unwrap();
        host::casper_print(&format!("existing_entry={:?}", msg));

        host::casper_write(Keyspace::Context(b"read back"), msg.as_bytes()).unwrap();

        const PARAM_1: &str = "param_1";
        const PARAM_2: &str = "param_2";

        let _params = [
            Param {
                name_ptr: PARAM_1.as_ptr(),
                name_len: PARAM_1.len(),
            },
            Param {
                name_ptr: PARAM_2.as_ptr(),
                name_len: PARAM_2.len(),
            },
        ];

        extern "C" fn mangled_entry_point_wrapper_1() {
            host::start(|(name, value): (String, u32)| {
                host::casper_print(&format!("Hello, world! Name={:?} value={:?}", name, value));
            });
        }

        extern "C" fn mangled_entry_point_wrapper_2() {
            // mangled_entry_point(param_1, param_2)
            host::casper_print("called inside a mangled entry point 2");
        }

        let param_1 = "param_1";
        let param_2 = "param_2";

        let params = [
            Param {
                name_ptr: param_1.as_ptr(),
                name_len: param_1.len(),
            },
            Param {
                name_ptr: param_2.as_ptr(),
                name_len: param_2.len(),
            },
        ];

        type _Foo = extern "C" fn() -> ();

        // host::casper_print(&format!("{:?}", mem::size_of::<
        let entry_point_1 = EntryPoint {
            selector: MANGLED_ENTRY_POINT_1.get(),

            fptr: mangled_entry_point_wrapper_1,
            flags: EntryPointFlags::empty().bits(),
        };

        let entry_point_2 = EntryPoint {
            selector: MANGLED_ENTRY_POINT_2.get(),

            fptr: mangled_entry_point_wrapper_2,
            flags: EntryPointFlags::empty().bits(),
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
        let res = host::casper_create(None, &manifest, 0, None, None);
        host::casper_print(&format!("create res {:?}", res));

        match res {
            Ok(CreateResult {
                package_address,
                contract_address,
                version,
            }) => {
                host::casper_print("success");

                let input_data = borsh::to_vec(&("Hello, world!", 42)).unwrap();

                let (data, result_code) =
                    host::casper_call(&contract_address, 10, MANGLED_ENTRY_POINT_1, &input_data);

                match result_code {
                    Ok(()) => {
                        host::casper_print(&format!("✅ Call succeeded. Output: {:?}", data));
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

    use casper_sdk::{
        host::native::{self, Environment},
        Contract,
    };

    use super::*;

    #[test]
    fn test() {
        let args = ("hello".to_string(), 123);
        native::dispatch_with(Environment::default().with_input_data(args), || {
            native::call_export("call");
        });
    }

    #[test]
    fn exports() {
        dbg!(native::list_exports());
        // dbg!(schema_helpe)
    }
}
