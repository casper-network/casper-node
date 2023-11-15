#![cfg_attr(target_arch = "wasm32", no_main)]
#![cfg_attr(target_arch = "wasm32", no_std)]

#[macro_use]
extern crate alloc;

use alloc::vec::Vec;
use casper_macros::{casper, Contract};
use casper_sdk::Value;

#[derive(Contract)]
struct Flipper {
    flag: Value<bool>,
}

#[casper(entry_points)]
impl Flipper {
    fn flip(&mut self) {
        let mut value = self.flag.get().unwrap().unwrap_or_default();
        value = !value;
        self.flag.set(value).unwrap();
    }

    fn flag_value(&self) -> bool {
        self.flag.get().unwrap().unwrap_or_default()
    }
}

// extern "C" fn flip(arg1: *const Slice, arg2: *const Slice);

mod exports {

    use alloc::{string::String, vec::Vec};
    use casper_macros::casper;
    use casper_sdk::{
        host::{self, EntryPoint, Manifest, Param},
        reserve_vec_space,
    };
    use core::ptr::NonNull;

    // use crate::reserve_vec_space;

    const KEY_SPACE_DEFAULT: u64 = 0;
    const TAG_BYTES: u64 = 0;

    #[casper(export)]
    pub fn call2() {
        unreachable!();
    }

    #[casper(export)]
    pub fn call(arg1: String, arg2: u32) {
        // (arg1, arg2): (String, u32) = Deserialize(input);
        host::print(&format!("arg1: {arg1}"));
        host::print(&format!("arg2: {arg2}"));

        // create_contract()

        // host::print(&format!(
        //     "arg1={:?} arg2={:?} arg3={:?}",
        //     core::str::from_utf8(arg1),
        //     core::str::from_utf8(arg2),
        //     core::str::from_utf8(arg3)
        // ));

        let mut read1 = Vec::new();

        let _non_existing_entry = host::read(KEY_SPACE_DEFAULT, b"hello", |size| {
            host::print(&format!("first cb alloc cb with size={size}"));
            reserve_vec_space(&mut read1, size)
            // static_buffer.as_mut_ptr()
        })
        .expect("should read");
        // host::print(&format!("non_existing_entry={:?}", non_existing_entry));
        host::write(KEY_SPACE_DEFAULT, b"hello", TAG_BYTES, b"Hello, world!").unwrap();

        let mut read2 = Vec::new();
        let existing_entry = host::read(KEY_SPACE_DEFAULT, b"hello", |size| {
            host::print(&format!("second cb alloc cb with size={size}"));
            reserve_vec_space(&mut read2, size)
        })
        .expect("should read")
        .expect("should have entry");
        host::print(&format!("existing_entry={:?}", existing_entry));
        let msg = String::from_utf8(read2).unwrap();
        host::print(&format!("existing_entry={:?}", msg));

        host::write(KEY_SPACE_DEFAULT, b"read back", TAG_BYTES, msg.as_bytes()).unwrap();

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
            // mangled_entry_point(param_1, param_2)
            host::print("called inside a mangled entry point 1");
        }

        extern "C" fn mangled_entry_point_wrapper_2() {
            // mangled_entry_point(param_1, param_2)
            host::print("called inside a mangled entry point 2");
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

        // host::print(&format!("{:?}", mem::size_of::<
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

        host::print(&format!(
            "Foo size/align {}/{}",
            core::mem::size_of::<*const [Param]>(),
            core::mem::size_of::<*const Param>()
        ));

        host::print(&format!(
            "{:?} (sz={})",
            &entry_points,
            core::mem::size_of::<EntryPoint>()
        ));

        let manifest = Manifest {
            entry_points: NonNull::from(entry_points.as_slice()).as_ptr(),
            entry_points_size: entry_points.len(),
        };
        host::print(&format!("manifest {:?}", manifest));
        let res = host::create(None, &manifest);
        host::print(&format!("create res {:?}", res));
        // let fptr_void: *const core::ffi::c_void = mangled_entry_point_wrapper as _;

        // let entry_point = EntryPoint {
        //     name_ptr: NAME.as_ptr(),
        //     name_len: NAME.len(),
        //     params_ptr: params.as_ptr(),
        //     params_size: params.len(),
        //     fptr: fptr_void,
        // };
        // host::print(&format!("{entry_point:?}"));
        // host::revert(123);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    todo!()
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
        let schema = Flipper::schema();
        assert_eq!(schema.name, "Flipper");
        assert_eq!(schema.entry_points[0].name, "flip");
        // assert_eq!(schema.entry_points[0].name, "flip");
        assert_eq!(schema.entry_points[1].name, "flag_value");
        // let s = serde_json::to_string_pretty(&schema).expect("foo");
        // println!("{s}");
    }

    #[test]
    fn should_flip() {
        assert_eq!(Flipper::name(), "Flipper");
        let mut flipper = Flipper::new();
        assert_eq!(flipper.flag_value(), false);
        flipper.flip();
        assert_eq!(flipper.flag_value(), true);
    }
}
