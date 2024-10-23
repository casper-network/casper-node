use std::{
    collections::{BTreeMap, BTreeSet},
    iter,
};

use blake2::{
    digest::{Update, VariableOutput},
    VarBlake2b,
};
use casper_execution_engine::engine_state::engine_config::DEFAULT_ENABLE_ENTITY;
use num_rational::Ratio;

use casper_storage::{
    data_access_layer::TransferRequest,
    system::runtime_native::{Config as NativeRuntimeConfig, TransferConfig},
};
use casper_types::{
    account::AccountHash,
    bytesrepr::ToBytes,
    system::mint::{ARG_AMOUNT, ARG_ID, ARG_SOURCE, ARG_TARGET},
    BlockTime, CLValue, Digest, FeeHandling, Gas, InitiatorAddr, ProtocolVersion, RefundHandling,
    RuntimeArgs, TransactionHash, TransactionV1Hash, TransferTarget, URef,
    DEFAULT_GAS_HOLD_INTERVAL, U512,
};

use crate::{
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_PUBLIC_KEY, DEFAULT_BLOCK_TIME, DEFAULT_PROTOCOL_VERSION,
};

/// Builds a [`TransferRequest`].
#[derive(Debug)]
pub struct TransferRequestBuilder {
    config: NativeRuntimeConfig,
    state_hash: Digest,
    block_time: BlockTime,
    protocol_version: ProtocolVersion,
    transaction_hash: Option<TransactionHash>,
    initiator: InitiatorAddr,
    authorization_keys: BTreeSet<AccountHash>,
    args: BTreeMap<String, CLValue>,
    gas: Gas,
}

impl TransferRequestBuilder {
    /// The default value used for `TransferRequest::config`.
    pub const DEFAULT_CONFIG: NativeRuntimeConfig = NativeRuntimeConfig::new(
        TransferConfig::Unadministered,
        FeeHandling::PayToProposer,
        RefundHandling::Refund {
            refund_ratio: Ratio::new_raw(99, 100),
        },
        0,
        true,
        true,
        0,
        500_000_000_000,
        DEFAULT_GAS_HOLD_INTERVAL.millis(),
        false,
        Ratio::new_raw(U512::zero(), U512::zero()),
        DEFAULT_ENABLE_ENTITY,
        2_500_000_000,
    );
    /// The default value used for `TransferRequest::state_hash`.
    pub const DEFAULT_STATE_HASH: Digest = Digest::from_raw([1; 32]);
    /// The default value used for `TransferRequest::gas`.
    pub const DEFAULT_GAS: u64 = 2_500_000_000;

    /// Constructs a new `TransferRequestBuilder`.
    pub fn new<A: Into<U512>, T: Into<TransferTarget>>(amount: A, target: T) -> Self {
        let mut args = BTreeMap::new();
        let _ = args.insert(
            ARG_AMOUNT.to_string(),
            CLValue::from_t(amount.into()).unwrap(),
        );
        let _ = args.insert(
            ARG_ID.to_string(),
            CLValue::from_t(Option::<u64>::None).unwrap(),
        );
        let target_value = match target.into() {
            TransferTarget::PublicKey(public_key) => CLValue::from_t(public_key),
            TransferTarget::AccountHash(account_hash) => CLValue::from_t(account_hash),
            TransferTarget::URef(uref) => CLValue::from_t(uref),
        }
        .unwrap();
        let _ = args.insert(ARG_TARGET.to_string(), target_value);
        TransferRequestBuilder {
            config: Self::DEFAULT_CONFIG,
            state_hash: Self::DEFAULT_STATE_HASH,
            block_time: BlockTime::new(DEFAULT_BLOCK_TIME),
            protocol_version: DEFAULT_PROTOCOL_VERSION,
            transaction_hash: None,
            initiator: InitiatorAddr::PublicKey(DEFAULT_ACCOUNT_PUBLIC_KEY.clone()),
            authorization_keys: iter::once(*DEFAULT_ACCOUNT_ADDR).collect(),
            args,
            gas: Gas::new(Self::DEFAULT_GAS),
        }
    }

    /// Sets the native runtime config of the [`TransferRequest`].
    pub fn with_native_runtime_config(mut self, config: NativeRuntimeConfig) -> Self {
        self.config = config;
        self
    }

    /// Sets the block time of the [`TransferRequest`].
    pub fn with_block_time(mut self, block_time: u64) -> Self {
        self.block_time = BlockTime::new(block_time);
        self
    }

    /// Sets the protocol version used by the [`TransferRequest`].
    pub fn with_protocol_version(mut self, protocol_version: ProtocolVersion) -> Self {
        self.protocol_version = protocol_version;
        self
    }

    /// Sets the transaction hash used by the [`TransferRequest`].
    pub fn with_transaction_hash(mut self, transaction_hash: TransactionHash) -> Self {
        self.transaction_hash = Some(transaction_hash);
        self
    }

    /// Sets the initiator used by the [`TransferRequest`], and adds its account hash to the set of
    /// authorization keys.
    pub fn with_initiator<T: Into<InitiatorAddr>>(mut self, initiator: T) -> Self {
        self.initiator = initiator.into();
        let _ = self
            .authorization_keys
            .insert(self.initiator.account_hash());
        self
    }

    /// Sets the authorization keys used by the [`TransferRequest`].
    pub fn with_authorization_keys<T: IntoIterator<Item = AccountHash>>(
        mut self,
        authorization_keys: T,
    ) -> Self {
        self.authorization_keys = authorization_keys.into_iter().collect();
        self
    }

    /// Adds the "source" runtime arg, replacing the existing one if it exists.
    pub fn with_source(mut self, source: URef) -> Self {
        let value = CLValue::from_t(source).unwrap();
        let _ = self.args.insert(ARG_SOURCE.to_string(), value);
        self
    }

    /// Adds the "id" runtime arg, replacing the existing one if it exists..
    pub fn with_transfer_id(mut self, id: u64) -> Self {
        let value = CLValue::from_t(Some(id)).unwrap();
        let _ = self.args.insert(ARG_ID.to_string(), value);
        self
    }

    /// Consumes self and returns a `TransferRequest`.
    ///
    /// If a transaction hash was not provided, the blake2b hash of the contents of the other fields
    /// will be calculated, so that different requests will have different transaction hashes.  Note
    /// that this generated hash is not the same as what would have been generated on an actual
    /// `Transaction` for an equivalent request.
    pub fn build(self) -> TransferRequest {
        let txn_hash = match self.transaction_hash {
            Some(txn_hash) => txn_hash,
            None => {
                let mut result = [0; 32];
                let mut hasher = VarBlake2b::new(32).unwrap();

                match &self.config.transfer_config() {
                    TransferConfig::Administered {
                        administrative_accounts,
                        allow_unrestricted_transfers,
                    } => hasher.update(
                        (administrative_accounts, allow_unrestricted_transfers)
                            .to_bytes()
                            .unwrap(),
                    ),
                    TransferConfig::Unadministered => {
                        hasher.update([1]);
                    }
                }
                hasher.update(self.config.fee_handling().to_bytes().unwrap());
                hasher.update(self.config.refund_handling().to_bytes().unwrap());
                hasher.update(
                    self.config
                        .vesting_schedule_period_millis()
                        .to_bytes()
                        .unwrap(),
                );
                hasher.update(self.config.allow_auction_bids().to_bytes().unwrap());
                hasher.update(self.config.compute_rewards().to_bytes().unwrap());
                hasher.update(
                    self.config
                        .max_delegators_per_validator()
                        .to_bytes()
                        .unwrap(),
                );
                hasher.update(self.config.minimum_delegation_amount().to_bytes().unwrap());
                hasher.update(self.state_hash);
                hasher.update(self.block_time.to_bytes().unwrap());
                hasher.update(self.protocol_version.to_bytes().unwrap());
                hasher.update(self.initiator.to_bytes().unwrap());
                hasher.update(self.authorization_keys.to_bytes().unwrap());
                hasher.update(self.args.to_bytes().unwrap());
                hasher.update(self.gas.to_bytes().unwrap());
                hasher.finalize_variable(|slice| {
                    result.copy_from_slice(slice);
                });
                TransactionHash::V1(TransactionV1Hash::from_raw(result))
            }
        };

        TransferRequest::with_runtime_args(
            self.config,
            self.state_hash,
            self.protocol_version,
            txn_hash,
            self.initiator,
            self.authorization_keys,
            RuntimeArgs::from(self.args),
        )
    }

    /// Sets the runtime args used by the [`TransferRequest`].
    ///
    /// NOTE: This is not generally useful for creating a valid `TransferRequest`, and hence is
    /// subject to change or deletion without notice.
    #[doc(hidden)]
    pub fn with_args(mut self, args: RuntimeArgs) -> Self {
        self.args = args
            .named_args()
            .map(|named_arg| (named_arg.name().to_string(), named_arg.cl_value().clone()))
            .collect();
        self
    }
}
