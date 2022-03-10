#![no_std]
#![no_main]

extern crate alloc;

use alloc::{boxed::Box, format, string::ToString, vec};

use casper_contract::{
    contract_api::{account, runtime, storage, system},
    unwrap_or_revert::UnwrapOrRevert,
};
use casper_types::{
    contracts::NamedKeys, ApiError, CLType, EntryPoint, EntryPointAccess, EntryPointType,
    EntryPoints, Parameter, PublicKey, URef, U512,
};

#[repr(u16)]
enum InstallerSessionError {
    FailedToTransfer = 101,
}

#[no_mangle]
pub extern "C" fn call_faucet() {
    faucet::delegate();
}

fn build_named_keys_and_purse() -> (NamedKeys, URef) {
    let mut named_keys = NamedKeys::new();
    let purse = system::create_purse();
    named_keys.insert(faucet::FAUCET_PURSE.to_string(), purse.into());

    named_keys.insert(faucet::INSTALLER.to_string(), runtime::get_caller().into());
    named_keys.insert(
        faucet::TIME_INTERVAL.to_string(),
        storage::new_uref(faucet::TWO_HOURS_AS_MILLIS).into(),
    );
    named_keys.insert(
        faucet::LAST_DISTRIBUTION_TIME.to_string(),
        storage::new_uref(0u64).into(),
    );
    named_keys.insert(
        faucet::AVAILABLE_AMOUNT.to_string(),
        storage::new_uref(U512::zero()).into(),
    );
    named_keys.insert(
        faucet::REMAINING_REQUESTS.to_string(),
        storage::new_uref(U512::zero()).into(),
    );
    named_keys.insert(
        faucet::DISTRIBUTIONS_PER_INTERVAL.to_string(),
        storage::new_uref(0u64).into(),
    );

    // TODO: document this and rename it to be more clear that this is the *only* authorized caller.
    // explain why it's useful to tell the contract who installed it / who is authorized to call it.
    // explain how the installer is authorized to change variables.
    // explain why it's more efficient to just build the named keys in the installer vs lazily
    // eval'd in the contract.
    named_keys.insert(
        faucet::AUTHORIZED_ACCOUNT.to_string(),
        storage::new_uref(None::<PublicKey>).into(),
    );

    (named_keys, purse)
}

#[no_mangle]
pub extern "C" fn call() {
    let id: u64 = runtime::get_named_arg(faucet::ARG_ID);

    let entry_points = {
        let mut entry_points = EntryPoints::new();

        let faucet = EntryPoint::new(
            faucet::ENTRY_POINT_FAUCET,
            vec![
                Parameter::new(faucet::ARG_ID, CLType::Option(Box::new(CLType::U64))),
                Parameter::new(faucet::ARG_TARGET, CLType::PublicKey),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        let set_variables = EntryPoint::new(
            faucet::ENTRY_POINT_SET_VARIABLES,
            vec![
                Parameter::new(
                    faucet::ARG_AVAILABLE_AMOUNT,
                    CLType::Option(Box::new(CLType::U512)),
                ),
                Parameter::new(
                    faucet::ARG_TIME_INTERVAL,
                    CLType::Option(Box::new(CLType::U64)),
                ),
                Parameter::new(
                    faucet::ARG_DISTRIBUTIONS_PER_INTERVAL,
                    CLType::Option(Box::new(CLType::U64)),
                ),
            ],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        let authorize_to = EntryPoint::new(
            faucet::ENTRY_POINT_AUTHORIZE_TO,
            vec![Parameter::new(
                faucet::ARG_TARGET,
                CLType::Option(Box::new(CLType::PublicKey)),
            )],
            CLType::Unit,
            EntryPointAccess::Public,
            EntryPointType::Contract,
        );

        entry_points.add_entry_point(faucet);
        entry_points.add_entry_point(set_variables);
        entry_points.add_entry_point(authorize_to);

        entry_points
    };

    // The installer will create the faucet purse and give it to the newly installed
    // contract so that the installing account and the newly installed account will
    // have a handle on it via the shared purse URef.
    //
    // The faucet named keys include the faucet purse, these are the named keys that we pass to the
    // faucet.
    let (faucet_named_keys, faucet_purse) = build_named_keys_and_purse();

    // This is where the contract package is created and the first version of the faucet contract is
    // installed within it. The contract package hash for the created contract package will be
    // stored in the installing account's named keys under the faucet::PACKAGE_HASH_KEY_NAME, this
    // allows later usage via the installing account to easily refer to and access the contract
    // package and thus all versions stored in it.
    //
    // The access URef for the contract package will also be stored in the installing account's
    // named keys under faucet::ACCESS_KEY_NAME; this URef controls administrative access to the
    // contract package which includes the ability to install new versions of the contract
    // logic, administer group-based security (if any), and so on.
    //
    // The installing account may decide to grant this access uref to another account (not
    // demonstrated here), which would allow that account equivalent full administrative control
    // over the contract. This should only be done intentionally because it is not revocable.
    let (contract_hash, contract_version) = storage::new_contract(
        entry_points,
        Some(faucet_named_keys),
        Some(faucet::HASH_KEY_NAME.to_string()),
        Some(faucet::ACCESS_KEY_NAME.to_string()),
    );

    // As a convenience, a specific contract version can be referred to either by its contract hash
    // or by the combination of the contract package hash and a contract version key. This comes
    // down to developer preference. The contract package hash is a stable hash, so this may be
    // preferable if you don't want to worry about contract hashes changing.
    // TODO: give ed's example of using an embedded contract hash vs a package hash, also using
    // parameterized package version. explain why this is an important opportunity to store the
    // urefs if you plan on ever plan on using this again.
    // TODO: namespace these using the id runtime arg.
    // TODO: explain why we're name-spacing like this.
    runtime::put_key(
        faucet::CONTRACT_VERSION,
        storage::new_uref(contract_version).into(),
    );
    runtime::put_key(faucet::CONTRACT_NAME, contract_hash.into());

    // This is specifically for this installing account, which would allow one installing account
    // to potentially have multiple faucet contract packages.
    runtime::put_key(&format!("faucet_{}", id), faucet_purse.into());

    let main_purse = account::get_main_purse();

    // Initial funding amount. In other words, when the faucet contract is set up, this is its
    // starting funds transferred from the installing account's main purse as a one-time
    // initialization.
    let amount = runtime::get_named_arg(ARG_AMOUNT);

    system::transfer_from_purse_to_purse(main_purse, faucet_purse, amount, Some(id))
        .unwrap_or_revert_with(ApiError::User(
            InstallerSessionError::FailedToTransfer as u16,
        ));
}
