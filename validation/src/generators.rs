use std::collections::BTreeMap;

use casper_types::{
    account::AccountHash,
    system::auction::{Bid, EraInfo, SeigniorageAllocation, UnbondingPurse},
    AccessRights, CLValue, DeployHash, DeployInfo, EraId, Key, NamedKey, PublicKey, SecretKey,
    Transfer, TransferAddr, Transform, URef, U128, U256, U512,
};
use casper_validation::{
    abi::{ABIFixture, ABITestCase},
    error::Error,
    Fixture, TestFixtures,
};

pub fn make_abi_test_fixtures() -> Result<TestFixtures, Error> {
    let basic = {
        let mut basic = BTreeMap::new();
        basic.insert(
            "SerializeU8".to_string(),
            ABITestCase::from_inputs(vec![254u8.into()])?,
        );
        basic.insert(
            "SerializeU16".to_string(),
            ABITestCase::from_inputs(vec![62356u16.into()])?,
        );
        basic.insert(
            "SerializeU32".to_string(),
            ABITestCase::from_inputs(vec![3490072870u32.into()])?,
        );
        basic.insert(
            "SerializeU64".to_string(),
            ABITestCase::from_inputs(vec![10829133186225377555u64.into()])?,
        );
        basic.insert(
            "SerializeString".to_string(),
            ABITestCase::from_inputs(vec!["Hello, world!".to_string().into()])?,
        );
        basic.insert(
            "SerializeBool".to_string(),
            ABITestCase::from_inputs(vec![true.into(), false.into()])?,
        );
        Fixture::ABI("basic".to_string(), ABIFixture::from(basic))
    };

    let transform = {
        let mut transform = BTreeMap::new();
        transform.insert(
            "Identity".to_string(),
            ABITestCase::from_inputs(vec![Transform::Identity.into()])?,
        );

        let write_string =
            CLValue::from_t("Hello, world!".to_string()).expect("should create clvalue of string");
        transform.insert(
            "WriteCLValue".to_string(),
            ABITestCase::from_inputs(vec![Transform::WriteCLValue(write_string).into()])?,
        );
        transform.insert(
            "WriteAccount".to_string(),
            ABITestCase::from_inputs(vec![
                Transform::WriteAccount(AccountHash::new([42; 32])).into()
            ])?,
        );
        transform.insert(
            "WriteContractWasm".to_string(),
            ABITestCase::from_inputs(vec![Transform::WriteContractWasm.into()])?,
        );
        transform.insert(
            "WriteContract".to_string(),
            ABITestCase::from_inputs(vec![Transform::WriteContract.into()])?,
        );
        transform.insert(
            "WriteContractPackage".to_string(),
            ABITestCase::from_inputs(vec![Transform::WriteContractPackage.into()])?,
        );

        let deploy_info = DeployInfo::new(
            DeployHash::new([55; 32]),
            &[TransferAddr::new([1; 32]), TransferAddr::new([2; 32])],
            AccountHash::new([100; 32]),
            URef::new([10; 32], AccessRights::READ_ADD_WRITE),
            U512::from(2_500_000_000u64),
        );
        transform.insert(
            "WriteDeployInfo".to_string(),
            ABITestCase::from_inputs(vec![Transform::WriteDeployInfo(deploy_info).into()])?,
        );

        let mut era_info = EraInfo::new();

        let validator_secret_key =
            SecretKey::ed25519_from_bytes([42; 32]).expect("should create secret key");
        let delegator_secret_key =
            SecretKey::secp256k1_from_bytes([43; 32]).expect("should create secret key");

        era_info
            .seigniorage_allocations_mut()
            .push(SeigniorageAllocation::Validator {
                validator_public_key: PublicKey::from(&validator_secret_key),
                amount: U512::from(1_000_000_000),
            });

        era_info
            .seigniorage_allocations_mut()
            .push(SeigniorageAllocation::Delegator {
                validator_public_key: PublicKey::from(&validator_secret_key),
                delegator_public_key: PublicKey::from(&delegator_secret_key),
                amount: U512::from(1_000_000_000),
            });
        transform.insert(
            "WriteEraInfo".to_string(),
            ABITestCase::from_inputs(vec![Transform::WriteEraInfo(era_info).into()])?,
        );

        let transfer = Transfer::new(
            DeployHash::new([44; 32]),
            AccountHash::new([100; 32]),
            Some(AccountHash::new([101; 32])),
            URef::new([10; 32], AccessRights::WRITE),
            URef::new([11; 32], AccessRights::WRITE),
            U512::from(15_000_000_000u64),
            U512::from(2_500_000_000u64),
            Some(1),
        );
        transform.insert(
            "WriteTransfer".to_string(),
            ABITestCase::from_inputs(vec![Transform::WriteTransfer(transfer).into()])?,
        );

        let bid = Bid::locked(
            PublicKey::from(&validator_secret_key),
            URef::new([10; 32], AccessRights::READ_ADD_WRITE),
            U512::from(50_000_000_000u64),
            100,
            u64::MAX,
        );
        transform.insert(
            "WriteBid".to_string(),
            ABITestCase::from_inputs(vec![Transform::WriteBid(Box::new(bid)).into()])?,
        );

        let unbonding_purse_1 = UnbondingPurse::new(
            URef::new([10; 32], AccessRights::READ),
            PublicKey::from(&validator_secret_key),
            PublicKey::from(&validator_secret_key),
            EraId::new(41),
            U512::from(60_000_000_000u64),
        );
        let unbonding_purse_2 = UnbondingPurse::new(
            URef::new([11; 32], AccessRights::READ),
            PublicKey::from(&validator_secret_key),
            PublicKey::from(&delegator_secret_key),
            EraId::new(42),
            U512::from(50_000_000_000u64),
        );

        transform.insert(
            "WriteWithdraw".to_string(),
            ABITestCase::from_inputs(vec![Transform::WriteWithdraw(vec![
                unbonding_purse_1,
                unbonding_purse_2,
            ])
            .into()])?,
        );
        transform.insert(
            "AddInt32".to_string(),
            ABITestCase::from_inputs(vec![Transform::AddInt32(i32::MAX).into()])?,
        );
        transform.insert(
            "AddUInt64".to_string(),
            ABITestCase::from_inputs(vec![Transform::AddUInt64(u64::MAX).into()])?,
        );
        transform.insert(
            "AddUInt128".to_string(),
            ABITestCase::from_inputs(vec![Transform::AddUInt128(U128::MAX).into()])?,
        );
        transform.insert(
            "AddUInt256".to_string(),
            ABITestCase::from_inputs(vec![Transform::AddUInt256(U256::MAX).into()])?,
        );
        transform.insert(
            "AddUInt512".to_string(),
            ABITestCase::from_inputs(vec![Transform::AddUInt512(U512::MAX).into()])?,
        );

        let named_keys = {
            let mut named_keys = Vec::new();
            let key_hash = Key::Hash([42; 32]);
            let key_uref = Key::URef(URef::new([43; 32], AccessRights::READ_ADD_WRITE));
            named_keys.push(NamedKey {
                name: "key 1".to_string(),
                key: key_hash.to_formatted_string(),
            });
            named_keys.push(NamedKey {
                name: "uref".to_string(),
                key: key_uref.to_formatted_string(),
            });
            named_keys
        };

        transform.insert(
            "AddKeys".to_string(),
            ABITestCase::from_inputs(vec![Transform::AddKeys(named_keys).into()])?,
        );
        transform.insert(
            "Failure".to_string(),
            ABITestCase::from_inputs(vec![Transform::Failure(
                "This is error message".to_string(),
            )
            .into()])?,
        );
        Fixture::ABI("transform".to_string(), ABIFixture::from(transform))
    };

    Ok(vec![basic, transform])
}
