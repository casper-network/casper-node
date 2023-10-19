use alloc::{string::String, vec::Vec};
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::TransactionV1;
use super::{NativeTransactionV1, UserlandTransactionV1};
#[cfg(any(feature = "std", test))]
use super::{TransactionConfig, TransactionV1ConfigFailure};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    account::AccountHash,
    bytesrepr::{self, Bytes, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    transaction::{AuctionTransactionV1, DirectCallV1},
    AddressableEntityHash, CLTyped, CLValueError, EntityVersion, PackageHash, PackageIdentifier,
    PublicKey, RuntimeArgs, URef, U512,
};

const NATIVE_TAG: u8 = 0;
const USERLAND_TAG: u8 = 1;

/// The high-level kind of a given [`TransactionV1`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "The high-level kind of a given TransactionV1.")
)]
#[serde(deny_unknown_fields)]
pub enum TransactionV1Kind {
    /// A transaction targeting native functionality.
    Native(NativeTransactionV1),
    /// A transaction with userland (i.e. not native) functionality.
    Userland(UserlandTransactionV1),
}

impl TransactionV1Kind {
    /// Returns a new transfer transaction body.
    pub fn new_transfer<A: Into<U512>>(
        source: URef,
        target: URef,
        amount: A,
        maybe_to: Option<AccountHash>,
        maybe_id: Option<u64>,
    ) -> Result<Self, CLValueError> {
        let txn = NativeTransactionV1::new_transfer(source, target, amount, maybe_to, maybe_id)?;
        Ok(TransactionV1Kind::Native(txn))
    }

    /// Returns a new add-bid transaction body.
    pub fn new_add_bid<A: Into<U512>>(
        public_key: PublicKey,
        delegation_rate: u8,
        amount: A,
    ) -> Result<Self, CLValueError> {
        let txn = AuctionTransactionV1::new_add_bid(public_key, delegation_rate, amount)?;
        Ok(TransactionV1Kind::Native(NativeTransactionV1::new_auction(
            txn,
        )))
    }

    /// Returns a new withdraw-bid transaction body.
    pub fn new_withdraw_bid<A: Into<U512>>(
        public_key: PublicKey,
        amount: A,
    ) -> Result<Self, CLValueError> {
        let txn = AuctionTransactionV1::new_withdraw_bid(public_key, amount)?;
        Ok(TransactionV1Kind::Native(NativeTransactionV1::new_auction(
            txn,
        )))
    }

    /// Returns a new delegate transaction body.
    pub fn new_delegate<A: Into<U512>>(
        delegator: PublicKey,
        validator: PublicKey,
        amount: A,
    ) -> Result<Self, CLValueError> {
        let txn = AuctionTransactionV1::new_delegate(delegator, validator, amount)?;
        Ok(TransactionV1Kind::Native(NativeTransactionV1::new_auction(
            txn,
        )))
    }

    /// Returns a new undelegate transaction body.
    pub fn new_undelegate<A: Into<U512>>(
        delegator: PublicKey,
        validator: PublicKey,
        amount: A,
    ) -> Result<Self, CLValueError> {
        let txn = AuctionTransactionV1::new_undelegate(delegator, validator, amount)?;
        Ok(TransactionV1Kind::Native(NativeTransactionV1::new_auction(
            txn,
        )))
    }

    /// Returns a new redelegate transaction body.
    pub fn new_redelegate<A: Into<U512>>(
        delegator: PublicKey,
        validator: PublicKey,
        amount: A,
        new_validator: PublicKey,
    ) -> Result<Self, CLValueError> {
        let txn =
            AuctionTransactionV1::new_redelegate(delegator, validator, amount, new_validator)?;
        Ok(TransactionV1Kind::Native(NativeTransactionV1::new_auction(
            txn,
        )))
    }

    /// Returns a new get-era-validators transaction body.
    pub fn new_get_era_validators() -> Self {
        let txn = AuctionTransactionV1::new_get_era_validators();
        TransactionV1Kind::Native(NativeTransactionV1::new_auction(txn))
    }

    /// Returns a new read-era-id transaction body.
    pub fn new_read_era_id() -> Self {
        let txn = AuctionTransactionV1::new_read_era_id();
        TransactionV1Kind::Native(NativeTransactionV1::new_auction(txn))
    }

    /// Returns a new reservation transaction body.
    pub fn new_reservation() -> Self {
        TransactionV1Kind::Native(NativeTransactionV1::new_reservation())
    }

    /// Returns a new standard userland transaction body.
    pub fn new_userland_standard(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        let txn = UserlandTransactionV1::new_standard(module_bytes, args);
        TransactionV1Kind::Userland(txn)
    }

    /// Returns a new installer transaction body.
    pub fn new_installer(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        let txn = UserlandTransactionV1::new_installer(module_bytes, args);
        TransactionV1Kind::Userland(txn)
    }

    /// Returns a new upgrader transaction body.
    pub fn new_upgrader(
        contract_package_id: PackageIdentifier,
        module_bytes: Bytes,
        args: RuntimeArgs,
    ) -> Self {
        let txn = UserlandTransactionV1::new_upgrader(contract_package_id, module_bytes, args);
        TransactionV1Kind::Userland(txn)
    }

    /// Returns a new stored-contract-by-hash transaction body.
    pub fn new_stored_contract_by_hash(
        hash: AddressableEntityHash,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        let txn = DirectCallV1::new_stored_contract_by_hash(hash, entry_point, args);
        TransactionV1Kind::Userland(UserlandTransactionV1::DirectCall(txn))
    }

    /// Returns a new stored-contract-by-name transaction body.
    pub fn new_stored_contract_by_name(
        name: String,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        let txn = DirectCallV1::new_stored_contract_by_name(name, entry_point, args);
        TransactionV1Kind::Userland(UserlandTransactionV1::DirectCall(txn))
    }

    /// Returns a new stored-versioned-contract-by-hash transaction body.
    pub fn new_stored_versioned_contract_by_hash(
        hash: PackageHash,
        version: Option<EntityVersion>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        let txn =
            DirectCallV1::new_stored_versioned_contract_by_hash(hash, version, entry_point, args);
        TransactionV1Kind::Userland(UserlandTransactionV1::DirectCall(txn))
    }

    /// Returns a new stored-versioned-contract-by-name transaction body.
    pub fn new_stored_versioned_contract_by_name(
        name: String,
        version: Option<EntityVersion>,
        entry_point: String,
        args: RuntimeArgs,
    ) -> Self {
        let txn =
            DirectCallV1::new_stored_versioned_contract_by_name(name, version, entry_point, args);
        TransactionV1Kind::Userland(UserlandTransactionV1::DirectCall(txn))
    }

    /// Returns a new no-op transaction body.
    pub fn new_noop(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        let txn = UserlandTransactionV1::new_noop(module_bytes, args);
        TransactionV1Kind::Userland(txn)
    }

    /// Returns a new closed transaction body.
    pub fn new_closed(module_bytes: Bytes, args: RuntimeArgs) -> Self {
        let txn = UserlandTransactionV1::new_closed(module_bytes, args);
        TransactionV1Kind::Userland(txn)
    }

    /// Returns the runtime arguments.
    pub fn args(&self) -> &RuntimeArgs {
        match self {
            TransactionV1Kind::Native(native_transaction) => native_transaction.args(),
            TransactionV1Kind::Userland(userland_transaction) => userland_transaction.args(),
        }
    }

    /// Returns the entry point name, if any.
    pub fn entry_point_name(&self) -> Option<&str> {
        match self {
            TransactionV1Kind::Native(_) => None,
            TransactionV1Kind::Userland(userland_transaction) => {
                Some(userland_transaction.entry_point_name())
            }
        }
    }

    /// Inserts a new runtime arg, regardless of the transaction kind.
    pub fn insert_arg<K, V>(&mut self, key: K, value: V) -> Result<(), CLValueError>
    where
        K: Into<String>,
        V: CLTyped + ToBytes,
    {
        let args = match self {
            TransactionV1Kind::Native(txn) => txn.args_mut(),
            TransactionV1Kind::Userland(txn) => txn.args_mut(),
        };
        args.insert(key, value)
    }

    #[cfg(any(feature = "std", test))]
    pub(super) fn has_valid_args(
        &self,
        config: &TransactionConfig,
    ) -> Result<(), TransactionV1ConfigFailure> {
        match self {
            TransactionV1Kind::Native(txn) => txn.has_valid_args(config),
            TransactionV1Kind::Userland(_) => Ok(()),
        }
    }

    #[cfg(any(feature = "std", test))]
    pub(super) fn module_bytes_is_present_but_empty(&self) -> bool {
        match self {
            TransactionV1Kind::Native(_) => false,
            TransactionV1Kind::Userland(txn) => txn.module_bytes_is_present_but_empty(),
        }
    }

    /// Returns a random `TransactionV1Kind`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..2) {
            0 => TransactionV1Kind::Native(NativeTransactionV1::random(rng)),
            1 => TransactionV1Kind::Userland(UserlandTransactionV1::random(rng)),
            _ => unreachable!(),
        }
    }
}

impl From<DirectCallV1> for TransactionV1Kind {
    fn from(direct_call: DirectCallV1) -> Self {
        TransactionV1Kind::Userland(UserlandTransactionV1::DirectCall(direct_call))
    }
}

impl From<AuctionTransactionV1> for TransactionV1Kind {
    fn from(auction_transaction: AuctionTransactionV1) -> Self {
        TransactionV1Kind::Native(NativeTransactionV1::Auction(auction_transaction))
    }
}

impl From<NativeTransactionV1> for TransactionV1Kind {
    fn from(native_transaction: NativeTransactionV1) -> Self {
        TransactionV1Kind::Native(native_transaction)
    }
}

impl From<UserlandTransactionV1> for TransactionV1Kind {
    fn from(userland_transaction: UserlandTransactionV1) -> Self {
        TransactionV1Kind::Userland(userland_transaction)
    }
}

impl Display for TransactionV1Kind {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            TransactionV1Kind::Native(txn) => write!(formatter, "kind: {}", txn),
            TransactionV1Kind::Userland(txn) => write!(formatter, "kind: {}", txn),
        }
    }
}

impl ToBytes for TransactionV1Kind {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            TransactionV1Kind::Native(txn) => {
                NATIVE_TAG.write_bytes(writer)?;
                txn.write_bytes(writer)
            }
            TransactionV1Kind::Userland(txn) => {
                USERLAND_TAG.write_bytes(writer)?;
                txn.write_bytes(writer)
            }
        }
    }

    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        U8_SERIALIZED_LENGTH
            + match self {
                TransactionV1Kind::Native(txn) => txn.serialized_length(),
                TransactionV1Kind::Userland(txn) => txn.serialized_length(),
            }
    }
}

impl FromBytes for TransactionV1Kind {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            NATIVE_TAG => {
                let (txn, remainder) = NativeTransactionV1::from_bytes(remainder)?;
                Ok((TransactionV1Kind::Native(txn), remainder))
            }
            USERLAND_TAG => {
                let (txn, remainder) = UserlandTransactionV1::from_bytes(remainder)?;
                Ok((TransactionV1Kind::Userland(txn), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&TransactionV1Kind::random(rng));
    }
}
