use casper_contract_macros::casper;

/// Deposit interface for contracts to receive tokens.
///
/// Useful for contracts that need to receive tokens.
#[casper]
pub trait Deposit {
    /// Deposit tokens into the contract.
    #[casper(payable)]
    fn deposit(&mut self);
}

#[casper]
pub trait SupportsALotOfArguments {
    fn very_long_list_of_arguments(
        &mut self,
        a0: u64,
        a1: u64,
        a2: u64,
        a3: u64,
        a4: String,
        a5: String,
        a6: u64,
        a7: u64,
        a8: u64,
        a9: u64,
        a10: u32,
        a11: u16,
        a12: String,
        a13: bool,
        a14: u32,
        a15: Vec<String>,
        a16: Vec<u64>,
        a17: String,
        a18: String,
        a19: Option<String>,
        a20: u64,
        a21: u32,
        a22: (u64, u32, u16, u8),
        a23: (String, String, String, String, u64),
    );
}
