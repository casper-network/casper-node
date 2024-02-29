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

pub fn sec_check(_allowed_badge_list: &[SecurityBadge]) {
    let _caller = host::get_caller();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec_check_works() {
        sec_check(&[SecurityBadge::Admin]);
    }
}
