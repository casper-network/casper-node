use casper_sdk::{
    host::native::{dispatch_with, with_stub, Stub},
    Contract,
};
use vm2_cep18::CEP18;

mod bindings {
    include!(concat!(env!("OUT_DIR"), "/cep18_schema.rs"));
}

#[test]
fn foo() {
    let stub = Stub::default();

    let ret = dispatch_with(stub, || {
        let client = bindings::CEP18Client::new::<CEP18>().expect("Constructor should work");
        let transfer_result = client
            .transfer([1; 32], 42)
            .expect("Calling transfer entry point should work");
        let result = transfer_result.into_result();
        dbg!(&result);
    });

    assert_eq!(ret, Ok(()));
}
