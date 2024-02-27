use casper_engine_test_support::{
    auction, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
};
use casper_types::{
    runtime_args, system::mint::TOTAL_SUPPLY_KEY, Key, RuntimeArgs, URef,
    U512,
};
use tempfile::TempDir;

const TEST_DELEGATOR_INITIAL_ACCOUNT_BALANCE: u64 = 1_000_000 * 1_000_000_000;
const CONTRACT_CREATE_PURSES: &str = "create_purses.wasm";
const CONTRACT_BURN: &str = "burn.wasm";

const ARG_AMOUNT: &str = "amount";
const ARG_SEED_AMOUNT: &str = "seed_amount";
const ARG_TOTAL_PURSES: &str = "total_purses";

const ARG_PURSE: &str = "purse";

#[ignore]
#[test]
fn should_burn_tokens_from_provided_purse() {
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.as_ref());
    let purse_amount = U512::from(5000000000u64);
    let total_purses = 2u64;
    let source = DEFAULT_ACCOUNT_ADDR.clone();

    let delegator_keys = auction::generate_public_keys(1);
    let validator_keys = auction::generate_public_keys(1);

    auction::run_genesis_and_create_initial_accounts(
        &mut builder,
        &validator_keys,
        delegator_keys
            .iter()
            .map(|public_key| public_key.to_account_hash())
            .collect::<Vec<_>>(),
        U512::from(TEST_DELEGATOR_INITIAL_ACCOUNT_BALANCE),
    );

    let exec_request = ExecuteRequestBuilder::standard(
        source,
        CONTRACT_CREATE_PURSES,
        runtime_args! {
            ARG_AMOUNT => U512::from(total_purses) * purse_amount,
            ARG_TOTAL_PURSES => total_purses,
            ARG_SEED_AMOUNT => purse_amount
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    // Return created purses for given account by filtering named keys
    let query_result = builder
        .query(None, Key::Account(source), &[])
        .expect("should query target");
    let account = query_result
        .as_account()
        .unwrap_or_else(|| panic!("result should be account but received {:?}", query_result));

    let urefs: Vec<URef> = (0..total_purses)
        .map(|index| {
            let purse_lookup_key = format!("purse:{}", index);
            let purse_uref = account
                .named_keys()
                .get(&purse_lookup_key)
                .and_then(Key::as_uref)
                .unwrap_or_else(|| panic!("should get named key {} as uref", purse_lookup_key));
            *purse_uref
        })
        .collect();

    assert_eq!(urefs.len(), 2);

    for uref in &urefs {
        let balance = builder
            .get_purse_balance_result(uref.clone())
            .motes()
            .cloned()
            .unwrap();

        assert_eq!(balance, purse_amount);
    }

    let total_supply_before_burning: U512 =
        builder.get_value(builder.get_mint_contract_hash(), TOTAL_SUPPLY_KEY);

    {
        let exec_request = ExecuteRequestBuilder::standard(
            source,
            CONTRACT_BURN,
            runtime_args! {
                ARG_PURSE => urefs[0].clone(),
                ARG_AMOUNT => U512::MAX,
            },
        )
        .build();

        builder.exec(exec_request).expect_success().commit();

        assert_eq!(
            builder
                .get_purse_balance_result(urefs[0].clone())
                .motes()
                .cloned()
                .unwrap(),
            U512::zero()
        );
    }

    {
        let exec_request = ExecuteRequestBuilder::standard(
            source,
            CONTRACT_BURN,
            runtime_args! {
                ARG_PURSE => urefs[1].clone(),
                ARG_AMOUNT => purse_amount / 2
            },
        )
        .build();

        builder.exec(exec_request).expect_success().commit();

        assert_eq!(
            builder
                .get_purse_balance_result(urefs[1].clone())
                .motes()
                .cloned()
                .unwrap(),
            purse_amount / 2
        );
    }

    let total_supply_after_burning: U512 =
        builder.get_value(builder.get_mint_contract_hash(), TOTAL_SUPPLY_KEY);

    let total_supply_difference = total_supply_before_burning - total_supply_after_burning;

    assert_eq!(
        total_supply_difference,
        U512::from(total_purses) * purse_amount
    );
}
