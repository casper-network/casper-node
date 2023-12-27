use borsh::{BorshDeserialize, BorshSerialize};
use casper_macros::CasperABI;
use casper_sdk::host;

#[derive(Clone, Copy, PartialEq, Eq, Debug, BorshSerialize, BorshDeserialize, CasperABI)]
#[borsh(use_discriminant = true)]
pub enum SecurityBadge {
    Admin = 0,
    Minter = 1,
    None = 2,
}

pub(crate) fn sec_check(allowed_badge_list: &[SecurityBadge]) {
    let caller = host::get_caller();
}

#[cfg(test)]
mod tests {
    #[test]
    fn foo() {}
}
