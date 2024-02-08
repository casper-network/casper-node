use casper_sdk::{
    host::native::{dispatch_with, with_stub, Stub},
    Contract,
};
use casper_sdk_codegen::support::IntoResult;
use vm2_cep18::contract::CEP18;

mod bindings {
    include!(concat!(env!("OUT_DIR"), "/cep18_schema.rs"));
}

#[test]
fn foo() {
    let stub = Stub::default();

    let ret = dispatch_with(stub, || {
        let client = bindings::CEP18Client::new::<CEP18>("Token Name".to_string())
            .expect("Constructor should work");

        // Calling the `transfer` entry point with the following arguments:
        let transfer_call_result = client
            .transfer([1; 32], 42)
            .expect("Calling transfer entry point should work");

        assert!(!transfer_call_result.did_revert());

        // Actual returned data, deserialized from the returned bytes.
        let transfer_return_value = transfer_call_result.into_return_value();

        assert_eq!(
            transfer_return_value.clone(),
            bindings::Result_____vm2_cep18__error__Cep18Error_::Err(
                bindings::vm2_cep18__error__Cep18Error::InsufficientBalance(())
            )
        );

        // Codegen can convert into standard Result type.
        assert_eq!(
            transfer_return_value.into_result(),
            Err(bindings::vm2_cep18__error__Cep18Error::InsufficientBalance(
                ()
            ))
        );
    });

    assert_eq!(ret, Ok(()));
}
