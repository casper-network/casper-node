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

    // This session constructs a NamedKeys struct and later passes it to the
    // storage::new_contract() function. This is simpler and more efficient than creating a custom
    // "init" entry point for the stored contract in this case but it is not the best approach for
    // every case. If you need to use the values that are stored under a new contract's named keys,
    // you may store them under the named keys of the account that was used to deploy the session.
    // However, this is not the best solution for every case, and there is another option.
    //
    // A custom "init" entrypoint would be useful for setting each required
    // named key for the contract, but can only be called after the contract has been created
    // using storage::new_contract(). Other entry points in the stored contract may require
    // extra logic to check for the presence of, load and validate data stored under named keys.
    // This would be useful in cases where a stored contract is creating another contract using
    // storage::new_contract(), especially if values computed for initializing the new contract are
    // also needed by the contract doing the initializing.
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

    // The AUTHORIZED_ACCOUNT named key holds an optional public key. If the public key is set,
    // the account referenced by this public key will be granted a special privilege as the only
    // authorized caller of the faucet's ENTRY_POINT_FAUCET.
    // Only the authorized account will be able to issue token distributions from the faucet. The
    // authorized account should call the faucet in the same way that the installer would,
    // passing faucet::ARG_AMOUNT, faucet::ARG_TARGET and faucet::ARG_ID runtime arguments.
    //
    // The AUTHORIZED_ACCOUNT and faucet installer account have different responsibilities. While
    // both of them may issue token using the ENTRY_POINT_FAUCET, only the faucet installer may
    // configure the contract through the ENTRY_POINT_SET_VARIABLES, and only the faucet installer
    // may set an authorized account through the ENTRY_POINT_AUTHORIZE_TO. The AUTHORIZED_ACCOUNT's
    // responsibility would be to determine to whom and what amount of token should be issued
    // through the faucet contract.
    //
    // While the AUTHORIZED_ACCOUNT named key is set to None::<PublicKey>, the ENTRY_POINT_FAUCET
    // will be publicly accessible and users may call ENTRY_POINT_FAUCET without a
    // faucet::ARG_TARGET or faucet::ARG_AMOUNT. The contract will automatically issue them a
    // computed amount of token.
    //
    // This enables the faucet contract to support a wider range of use cases, where in some cases
    // the faucet installer does not want the ENTRY_POINT_FAUCET to be called directly by users for
    // security reasons. Another case would be where this contract is deployed to a private Casper
    // Network where all users are trusted to use the faucet to issue themselves token
    // distributions responsibly.
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
                Parameter::new(faucet::ARG_AMOUNT, CLType::U512),
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
    // contract so that the installing account and the newly installed contract will
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
        Some(format!("{}_{}", faucet::HASH_KEY_NAME, id)),
        Some(format!("{}_{}", faucet::ACCESS_KEY_NAME, id)),
    );

    // As a convenience, a specific contract version can be referred to either by its contract hash
    // or by the combination of the contract package hash and a contract version key. This comes
    // down to developer preference. The contract package hash is a stable hash, so this may be
    // preferable if you don't want to worry about contract hashes changing. Existing contracts'
    // hashes can be stored under a URef under a named key and later used for calling the contract
    // by hash. If you wanted to change the hash stored under the named key, your contract would
    // have to have an entrypoint that would allow you to do so.
    //
    // Another option is to store a contract package hash that your contract depends on. When
    // calling a contract using the package hash alone, the execution engine will find the latest
    // contract version for you automatically. To avoid breaking changes, you may want to use a
    // contract package hash and a contract version. That way, whenever the contract package
    // that the calling contract depends on changes, the version can be updated through an
    // entrypoint that allows an authorized caller to set named keys.
    //
    // In some cases it may be desireable to pass one or both of the contract package hash and
    // version into contract or session code as a runtime argument. As an example, if a user
    // regularly makes calls to a contract package via session code, they could have the session
    // code take runtime arguments for one or both of the contract package hash and version. This
    // way, if the contract package is updated, they could easily use the latest version without
    // needing to edit their session code. The same technique could be applied to stored contracts
    // that need to call other contracts by their contract package hash and version.

    // Here we are saving newly created contracts hash, the contract package hash and contract
    // version, and an access URef under the installer's named keys. It's important to note that
    // you'll need the access URef if you ever want to modify the contract package in the future.
    // It's also important to note that it will be impossible to reference any of these values again
    // if they're not stored under named keys.
    //
    // These named keys all end with the "id" runtime argument that is passed into this session.
    // This is keep separate instances of this faucet contract namespaced in case the installer
    // wants to install multiple instances of the contract using the same account.
    runtime::put_key(
        &format!("{}_{}", faucet::CONTRACT_VERSION, id),
        storage::new_uref(contract_version).into(),
    );
    runtime::put_key(
        &format!("{}_{}", faucet::CONTRACT_NAME, id),
        contract_hash.into(),
    );

    // This is specifically for this installing account, which would allow one installing account
    // to potentially have multiple faucet contract packages.
    runtime::put_key(
        &format!("{}_{}", faucet::FAUCET_PURSE, id),
        faucet_purse.into(),
    );

    let main_purse = account::get_main_purse();

    // Initial funding amount. In other words, when the faucet contract is set up, this is its
    // starting tokens transferred from the installing account's main purse as a one-time
    // initialization.
    let amount = runtime::get_named_arg(faucet::ARG_AMOUNT);

    system::transfer_from_purse_to_purse(main_purse, faucet_purse, amount, Some(id))
        .unwrap_or_revert_with(ApiError::User(
            InstallerSessionError::FailedToTransfer as u16,
        ));
}
