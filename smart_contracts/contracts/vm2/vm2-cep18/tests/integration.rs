use casper_sdk::Contract;
use vm2_cep18::CEP18;

mod bindings {
    include!(concat!(env!("OUT_DIR"), "/cep18_schema.rs"));
}

#[test]
fn foo() {
    // CEP18::create(Some("new"), None).unwrap();
    let client = bindings::CEP18Client::new::<CEP18>().expect("Constructor should work");
    // let client = bindings::CEP18Client { address: [42; 32] };
    client.transfer([0; 32], 42);
}
