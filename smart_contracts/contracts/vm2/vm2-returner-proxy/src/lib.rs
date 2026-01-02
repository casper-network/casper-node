use casper_contract_sdk::{prelude::*, serializers::borsh, types::Address};
#[casper(contract_state)]
pub struct ReturnerProxyContract {
    address: Address,
}

impl Default for ReturnerProxyContract {
    fn default() -> Self {
        panic!("nope");
    }
}

#[casper]
impl ReturnerProxyContract {
    #[casper(constructor)]
    pub fn new(address: Address) -> Self {
        Self { address }
    }

    pub fn do_return(&self) -> u32 {
        let _ = casper::print(&format!(
            "Proxy trying to call do_return, address: {:?}",
            self.address
        ));
        let (data, res) = casper::casper_call(&self.address, 0, "do_return", &[], None, None);
        res.unwrap();
        borsh::from_slice(&data.unwrap()).unwrap()
    }

    pub fn do_not_return(&self) {
        let _ = casper::print(&format!(
            "Proxy trying to call do_not_return, address: {:?}",
            self.address
        ));
        _ = casper::casper_call(&self.address, 0, "do_not_return", &[], None, None);
    }
}
