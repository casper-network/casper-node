pub mod runtime;
pub mod types;

#[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
#[cfg(test)]
mod tests {
    /*#TODO fix native implementation
    use crate::{
        casper::native::{dispatch_with, Environment},
        compat::types::RuntimeArgs,
    };

    use super::*;
    use crate::compat::types::U512;

    #[test]
    fn test_compat() {
        let mut runtime_args = RuntimeArgs::new();
        runtime_args.insert("amount", U512::from(1000u64)).unwrap();
        runtime_args.insert("hello", String::from("world")).unwrap();
        runtime_args
            .insert("complex type", vec![1u64, 2u64, 3u64])
            .unwrap();

        let arg_bytes = borsh::to_vec(&runtime_args).unwrap();

        let env = Environment::default().with_input_data(arg_bytes);

        dispatch_with(env, || {
            let amount: U512 = runtime::get_named_arg("amount");
            assert_eq!(amount, U512::from(1000u64));
        })
        .expect("Dispatch failed");
    }
    */
}
