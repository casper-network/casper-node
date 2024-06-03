use casper_macros::casper;
use casper_sdk::host;

#[derive(PartialEq, Debug)]
#[casper]
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
