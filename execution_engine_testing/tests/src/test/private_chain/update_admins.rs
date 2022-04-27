use std::ops::Deref;

use casper_engine_test_support::{UpgradeRequestBuilder, DEFAULT_PROTOCOL_VERSION};
use casper_execution_engine::core::engine_state::genesis::AdministratorAccount;
use casper_types::{account::Weight, Motes, ProtocolVersion, PublicKey, SecretKey, SemVer};
use itertools::Itertools;

use super::{ADMIN_ACCOUNT_INITIAL_BALANCE, PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS};

#[ignore]
#[test]
fn should_update_list_of_admins_on_private_chain() {
    let mut builder = super::private_chain_setup();

    let new_admin_public_key = {
        let new_admin_secret_key = SecretKey::secp256k1_from_bytes(&[98; 32])
            .expect("should create new secp256k1 secret key");
        PublicKey::from(&new_admin_secret_key)
    };

    let new_admins = {
        let mut admins = PRIVATE_CHAIN_GENESIS_ADMIN_ACCOUNTS.clone();
        admins.push(AdministratorAccount::new(
            new_admin_public_key,
            Motes::new(ADMIN_ACCOUNT_INITIAL_BALANCE),
            Weight::MAX,
        ));

        assert!(
            admins.iter().map(|admin| admin.public_key()).all_unique(),
            "should have unique public keys"
        );

        admins
    };

    let protocol_version = *DEFAULT_PROTOCOL_VERSION;
    let new_protocol_version = ProtocolVersion::new(SemVer::new(
        protocol_version.value().major,
        protocol_version.value().minor,
        protocol_version.value().patch + 1,
    ));

    let mut upgrade_request = UpgradeRequestBuilder::new()
        .with_administrative_accounts(new_admins.clone())
        .with_current_protocol_version(protocol_version)
        .with_new_protocol_version(new_protocol_version)
        .build();

    builder.upgrade_with_upgrade_request(None, &mut upgrade_request);
    builder.expect_upgrade_success();

    assert_eq!(
        builder
            .get_engine_state()
            .config()
            .administrative_accounts()
            .deref(),
        &new_admins,
        "private chain genesis has administrator accounts defined"
    );
}
