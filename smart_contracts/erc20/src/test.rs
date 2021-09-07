use casper_types::URef;

struct ERC20 {
    balances: URef,
    allowances: URef,
}

impl Default for ERC20 {
    fn default() -> Self {
        // Self { balances: Default::default(), allowances: Default::default() }
        todo!()
    }
}

// impl ERC20 {
//     fn transfer
// }
