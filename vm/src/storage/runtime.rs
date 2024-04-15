use std::{cell::RefCell, rc::Rc, sync::Arc};

use casper_storage::{
    system::runtime_native::{Config, Id},
    AddressGenerator,
};
use casper_types::{
    account::AccountHash,
    addressable_entity::{NamedKeyAddr, NamedKeys},
    AddressableEntity, ContextAccessRights, EntityAddr, Key, ProtocolVersion, PublicKey, U512,
};
use parking_lot::RwLock;

use super::{GlobalStateReader, TrackingCopy};

/// A facade for the mint contract.
///
/// This struct provides a simplified interface to the mint contract.
pub struct RuntimeNative<R: GlobalStateReader> {
    entity_addr: EntityAddr,
    runtime_native: casper_storage::system::runtime_native::RuntimeNative<R>,
    // pub(crate) tracking_copy: TrackingCopy<R>,
    // pub(crate) address_generator: Arc<RwLock<AddressGenerator>>,
}
impl<R: GlobalStateReader> casper_storage::system::mint::runtime_provider::RuntimeProvider
    for RuntimeNative<R>
{
    fn get_caller(&self) -> AccountHash {
        self.runtime_native.get_caller()
    }

    fn get_immediate_caller(&self) -> Option<casper_types::system::Caller> {
        self.runtime_native.get_immediate_caller()
    }

    fn is_called_from_standard_payment(&self) -> bool {
        self.runtime_native.is_called_from_standard_payment()
    }

    fn get_system_entity_registry(
        &self,
    ) -> Result<casper_types::SystemEntityRegistry, casper_storage::system::error::ProviderError>
    {
        self.runtime_native.get_system_entity_registry()
    }

    fn read_addressable_entity_by_account_hash(
        &mut self,
        account_hash: AccountHash,
    ) -> Result<Option<AddressableEntity>, casper_storage::system::error::ProviderError> {
        self.runtime_native
            .read_addressable_entity_by_account_hash(account_hash)
    }

    fn get_phase(&self) -> casper_types::Phase {
        self.runtime_native.get_phase()
    }

    fn get_key(&mut self, name: &str) -> Option<casper_types::Key> {
        let named_key_addr = NamedKeyAddr::new_from_string(self.entity_addr, name.to_string())
            .expect("should create named key addr");
        let key = Key::NamedKey(named_key_addr);
        let stored_value = self
            .runtime_native
            .tracking_copy()
            .borrow_mut()
            .read(&key)
            .expect("should read named key")?;
        let named_key = stored_value.into_named_key().expect("should be named key");
        Some(named_key.get_key().expect("should get key"))
    }

    fn get_approved_spending_limit(&self) -> U512 {
        self.runtime_native.get_approved_spending_limit()
    }

    fn sub_approved_spending_limit(&mut self, amount: U512) {
        self.runtime_native.sub_approved_spending_limit(amount)
    }

    fn get_main_purse(&self) -> casper_types::URef {
        self.runtime_native.get_main_purse()
    }

    fn is_administrator(&self, account_hash: &AccountHash) -> bool {
        self.runtime_native.is_administrator(account_hash)
    }

    fn allow_unrestricted_transfers(&self) -> bool {
        self.runtime_native.allow_unrestricted_transfers()
    }
}

impl<R: GlobalStateReader> casper_storage::system::mint::system_provider::SystemProvider
    for RuntimeNative<R>
{
    fn record_transfer(
        &mut self,
        maybe_to: Option<AccountHash>,
        source: casper_types::URef,
        target: casper_types::URef,
        amount: U512,
        id: Option<u64>,
    ) -> Result<(), casper_types::system::mint::Error> {
        self.runtime_native
            .record_transfer(maybe_to, source, target, amount, id)
    }
}

impl<R: GlobalStateReader> casper_storage::system::mint::storage_provider::StorageProvider
    for RuntimeNative<R>
{
    fn new_uref<T: casper_types::CLTyped + casper_types::bytesrepr::ToBytes>(
        &mut self,
        init: T,
    ) -> Result<casper_types::URef, casper_types::system::mint::Error> {
        self.runtime_native.new_uref(init)
    }

    fn read<T: casper_types::CLTyped + casper_types::bytesrepr::FromBytes>(
        &mut self,
        uref: casper_types::URef,
    ) -> Result<Option<T>, casper_types::system::mint::Error> {
        self.runtime_native.read(uref)
    }

    fn write_amount(
        &mut self,
        uref: casper_types::URef,
        amount: U512,
    ) -> Result<(), casper_types::system::mint::Error> {
        self.runtime_native.write_amount(uref, amount)
    }

    fn add<T: casper_types::CLTyped + casper_types::bytesrepr::ToBytes>(
        &mut self,
        uref: casper_types::URef,
        value: T,
    ) -> Result<(), casper_types::system::mint::Error> {
        self.runtime_native.add(uref, value)
    }

    fn total_balance(
        &mut self,
        uref: casper_types::URef,
    ) -> Result<U512, casper_types::system::mint::Error> {
        self.runtime_native.total_balance(uref)
    }

    fn available_balance(
        &mut self,
        uref: casper_types::URef,
        holds_epoch: casper_types::HoldsEpoch,
    ) -> Result<Option<U512>, casper_types::system::mint::Error> {
        self.runtime_native.available_balance(uref, holds_epoch)
    }

    fn write_balance(
        &mut self,
        uref: casper_types::URef,
        balance: U512,
    ) -> Result<(), casper_types::system::mint::Error> {
        self.runtime_native.write_balance(uref, balance)
    }

    fn add_balance(
        &mut self,
        uref: casper_types::URef,
        value: U512,
    ) -> Result<(), casper_types::system::mint::Error> {
        self.runtime_native.add_balance(uref, value)
    }
}

impl<R: GlobalStateReader> casper_storage::system::mint::Mint for RuntimeNative<R> {}

impl<R: GlobalStateReader> RuntimeNative<R> {
    /// Creates a new instance of MintFacade.
    pub fn new(
        entity_addr: EntityAddr,
        config: Config,
        protocol_version: ProtocolVersion,
        id: Id,
        tracking_copy: TrackingCopy<R>,
        addressable_entity: AddressableEntity,
        access_rights: ContextAccessRights,
    ) -> Self {
        let tracking_copy = Rc::new(RefCell::new(tracking_copy));
        let remaining_spending_limit = U512::MAX; // NOTE: Since there's no custom payment, there's no need to track the remaining spending limit.
        let phase = casper_types::Phase::System;
        let named_keys = NamedKeys::new();
        let address = PublicKey::System.to_account_hash();
        let runtime_native = casper_storage::system::runtime_native::RuntimeNative::new(
            config,
            protocol_version,
            id,
            tracking_copy,
            address,
            addressable_entity,
            named_keys,
            access_rights,
            remaining_spending_limit,
            phase,
        );
        RuntimeNative {
            entity_addr,
            runtime_native,
        }
    }
}

#[cfg(test)]
mod tests {
    use casper_storage::{
        global_state::{self, state::StateProvider},
        system::mint::{storage_provider::StorageProvider, Mint},
    };
    use casper_types::{
        addressable_entity::NamedKeyValue, AccessRights, AddressableEntityHash, CLValue,
        HoldsEpoch, StoredValue, URef,
    };

    use super::*;

    #[test]
    fn test_mint() {
        let entity_addr = EntityAddr::SmartContract([111; 32]);
        let total_supply_uref = URef::new([44; 32], AccessRights::all());

        let named_key_addr = NamedKeyAddr::new_from_string(entity_addr, "total_supply".to_string())
            .expect("should create named key addr");
        let named_key_value = NamedKeyValue::new(
            CLValue::from_t(Key::URef(total_supply_uref)).unwrap(),
            CLValue::from_t("total_supply".to_string()).unwrap(),
        );

        let initial_data = [
            (
                Key::URef(total_supply_uref),
                StoredValue::CLValue(CLValue::from_t(U512::zero()).unwrap()),
            ),
            (
                Key::NamedKey(named_key_addr),
                StoredValue::NamedKey(named_key_value),
            ),
        ];

        let deploy_hash = [42; 32];

        let (global_state, root_hash, _tempdir) =
            global_state::state::lmdb::make_temporary_global_state(initial_data);

        let tracking_copy = global_state
            .tracking_copy(root_hash)
            .expect("Obtaining root hash succeed")
            .expect("Root hash exists");

        let mut runtime = RuntimeNative::new(
            entity_addr,
            Config::default(),
            ProtocolVersion::V1_0_0,
            Id::Seed(vec![1, 2, 3]),
            tracking_copy,
            AddressableEntity::new(
                Default::default(), // package_hash,
                Default::default(), // byte_code_hash,
                Default::default(), // entry_points,
                Default::default(), // protocol_version,
                Default::default(), // main_purse,
                Default::default(), // associated_keys,
                Default::default(), // action_thresholds,
                Default::default(), // message_topics,
                Default::default(), // entity_kind,
            ),
            ContextAccessRights::new(AddressableEntityHash::new([5; 32]), []),
        );

        let source = runtime.mint(U512::from(1000u64)).expect("Should mint");
        let source_balance = runtime
            .total_balance(source)
            .expect("Should get total balance");
        assert_eq!(source_balance, U512::from(1000u64));

        let target = runtime.mint(U512::from(1u64)).expect("Should create uref");
        let target_balance = runtime
            .total_balance(target)
            .expect("Should get total balance");
        assert_eq!(target_balance, U512::from(1u64));

        runtime
            .transfer(
                None,
                source,
                target,
                U512::from(999),
                None,
                HoldsEpoch::NOT_APPLICABLE,
            )
            .expect("Should transfer");

        let source_balance = runtime
            .total_balance(source)
            .expect("Should get total balance");
        assert_eq!(source_balance, U512::from(1u64));
        let target_balance = runtime
            .total_balance(target)
            .expect("Should get total balance");
        assert_eq!(target_balance, U512::from(1000u64));

        runtime
            .mint_into_existing_purse(target, U512::from(1000u64))
            .expect("Should mint");

        let target_balance = runtime
            .total_balance(target)
            .expect("Should get total balance");
        assert_eq!(target_balance, U512::from(2000u64));
    }
}
