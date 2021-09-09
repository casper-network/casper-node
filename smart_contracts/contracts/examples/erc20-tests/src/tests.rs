use casper_types::{Key, U256};

use crate::erc20::{token_cfg, Sender, Token};

#[test]
fn test_erc20_deploy() {
    let t = Token::deploy();
    assert_eq!(t.name(), token_cfg::NAME);
    assert_eq!(t.symbol(), token_cfg::SYMBOL);
    assert_eq!(t.decimals(), token_cfg::DECIMALS);
    assert_eq!(
        t.balance_of(Key::from(t.ali)),
        Some(token_cfg::total_supply())
    );
}

#[test]
fn test_erc20_transfer() {
    let transfer_amount_1 = U256::from(42);
    let transfer_amount_2 = U256::from(20);

    let mut t = Token::deploy();
    // ali -> bob
    assert_eq!(t.balance_of(Key::from(t.bob)), None);
    assert_eq!(
        t.balance_of(Key::from(t.ali)),
        Some(token_cfg::total_supply())
    );
    t.transfer(Key::from(t.bob), transfer_amount_1, Sender(t.ali));
    assert_eq!(t.balance_of(Key::from(t.bob)), Some(transfer_amount_1));
    assert_eq!(
        t.balance_of(Key::from(t.ali)),
        Some(token_cfg::total_supply() - transfer_amount_1)
    );

    // bob -> ali

    t.transfer(Key::from(t.ali), transfer_amount_2, Sender(t.bob));
    assert_eq!(
        t.balance_of(Key::from(t.ali)),
        Some(token_cfg::total_supply() - transfer_amount_1 + transfer_amount_2),
    );
    assert_eq!(
        t.balance_of(Key::from(t.bob)),
        Some(transfer_amount_1 - transfer_amount_2)
    );
}

#[test]
fn should_transfer_full_amount() {
    let mut t = Token::deploy();

    let initial_ali_balance = t.balance_of(Key::from(t.ali)).unwrap();
    assert_eq!(t.balance_of(Key::from(t.bob)), None);

    t.transfer(Key::from(t.bob), initial_ali_balance, Sender(t.ali));

    assert_eq!(t.balance_of(Key::from(t.bob)), Some(initial_ali_balance));
    assert_eq!(t.balance_of(Key::from(t.ali)), Some(U256::zero()));

    t.transfer(Key::from(t.ali), initial_ali_balance, Sender(t.bob));

    assert_eq!(t.balance_of(Key::from(t.bob)), Some(U256::zero()));
    assert_eq!(t.balance_of(Key::from(t.ali)), Some(initial_ali_balance));
}

#[should_panic(expected = "ApiError::User(65534) [131070]")]
#[test]
fn should_not_transfer_with_insufficient_balance() {
    let mut t = Token::deploy();

    let initial_ali_balance = t.balance_of(Key::from(t.ali)).unwrap();
    assert_eq!(t.balance_of(Key::from(t.bob)), None);

    t.transfer(
        Key::from(t.bob),
        initial_ali_balance + U256::one(),
        Sender(t.ali),
    );
}

#[test]
fn test_erc20_transfer_from() {
    // NOTE: exercises the happy path
    let approve_amount = U256::from(100);
    let transfer_amount = U256::from(42);
    assert!(approve_amount > transfer_amount);

    let mut t = Token::deploy();

    let owner = t.ali;
    let spender = t.bob;
    let recipient = t.joe;

    let owner_balance_before = t
        .balance_of(Key::from(owner))
        .expect("owner should have balance");
    t.approve(Key::from(spender), approve_amount, Sender(owner));
    assert_eq!(
        t.allowance(Key::from(owner), Key::from(spender)),
        Some(approve_amount)
    );

    t.transfer_from(
        Key::from(owner),
        Key::from(recipient),
        transfer_amount,
        Sender(spender),
    );

    assert_eq!(
        t.balance_of(Key::from(owner)),
        Some(owner_balance_before - transfer_amount),
        "should decrease balance of the owner"
    );
    assert_eq!(
        t.allowance(Key::from(owner), Key::from(spender)),
        Some(approve_amount - transfer_amount),
        "should decrease allowance of the spender"
    );
    assert_eq!(
        t.balance_of(Key::from(recipient)),
        Some(transfer_amount),
        "recipient should receive tokens"
    );
}

#[should_panic(expected = "ApiError::User(65533) [131069]")]
#[test]
fn test_should_not_transfer_from_more_than_approved() {
    // NOTE: exercises the happy path
    let approve_amount = U256::from(100);
    let transfer_amount = U256::from(42);
    assert!(approve_amount > transfer_amount);

    let mut t = Token::deploy();

    let owner = t.ali;
    let spender = t.bob;
    let recipient = t.joe;

    let owner_balance_before = t
        .balance_of(Key::from(owner))
        .expect("owner should have balance");
    t.approve(Key::from(spender), approve_amount, Sender(owner));
    assert_eq!(
        t.allowance(Key::from(owner), Key::from(spender)),
        Some(approve_amount)
    );

    t.transfer_from(
        Key::from(owner),
        Key::from(recipient),
        approve_amount + U256::one(),
        Sender(spender),
    );

    assert_eq!(
        t.balance_of(Key::from(owner)),
        Some(owner_balance_before - transfer_amount),
        "should decrease balance of the owner"
    );
    assert_eq!(
        t.allowance(Key::from(owner), Key::from(spender)),
        Some(approve_amount - transfer_amount),
        "should decrease allowance of the spender"
    );
    assert_eq!(
        t.balance_of(Key::from(recipient)),
        Some(transfer_amount),
        "recipient should receive tokens"
    );
}
