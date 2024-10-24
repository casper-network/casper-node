use super::tranasction_lane::{calculate_transaction_lane, TransactionLane};
use casper_types::{
    arg_handling, bytesrepr::ToBytes, crypto, Approval, Chainspec, Digest, DisplayIter, Gas,
    HashAddr, InitiatorAddr, InvalidTransaction, InvalidTransactionV1, PricingHandling,
    PricingMode, RuntimeArgs, TimeDiff, Timestamp, TransactionConfig, TransactionEntryPoint,
    TransactionScheduling, TransactionTarget, TransactionV1, TransactionV1ExcessiveSizeError,
    TransactionV1Hash, U512,
};
use core::fmt::{self, Debug, Display, Formatter};
#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "once_cell", test))]
use once_cell::sync::OnceCell;
use serde::Serialize;
use std::collections::BTreeSet;
use tracing::debug;

const ARGS_MAP_KEY: u16 = 0;
const TARGET_MAP_KEY: u16 = 1;
const ENTRY_POINT_MAP_KEY: u16 = 2;
const SCHEDULING_MAP_KEY: u16 = 3;
const EXPECTED_NUMBER_OF_FIELDS: usize = 4;

#[cfg_attr(feature = "datasize", derive(DataSize))]
#[derive(Clone, Debug, Serialize)]
pub struct MetaTransactionV1 {
    hash: TransactionV1Hash,
    chain_name: String,
    timestamp: Timestamp,
    ttl: TimeDiff,
    pricing_mode: PricingMode,
    initiator_addr: InitiatorAddr,
    args: RuntimeArgs,
    target: TransactionTarget,
    entry_point: TransactionEntryPoint,
    transaction_lane: TransactionLane,
    scheduling: TransactionScheduling,
    approvals: BTreeSet<Approval>,
    serialized_length: usize,
    payload_hash: Digest,
    has_valid_hash: Result<(), InvalidTransactionV1>,
    #[cfg_attr(any(all(feature = "std", feature = "once_cell"), test), serde(skip))]
    #[cfg_attr(
        all(any(feature = "once_cell", test), feature = "datasize"),
        data_size(skip)
    )]
    #[cfg(any(feature = "once_cell", test))]
    is_verified: OnceCell<Result<(), InvalidTransactionV1>>,
}

impl MetaTransactionV1 {
    pub fn from(
        v1: &TransactionV1,
        transaction_config: &TransactionConfig,
    ) -> Result<MetaTransactionV1, InvalidTransaction> {
        let args: RuntimeArgs = v1.deserialize_field(ARGS_MAP_KEY).map_err(|error| {
            InvalidTransaction::V1(InvalidTransactionV1::CouldNotDeserializeField { error })
        })?;
        let target: TransactionTarget = v1.deserialize_field(TARGET_MAP_KEY).map_err(|error| {
            InvalidTransaction::V1(InvalidTransactionV1::CouldNotDeserializeField { error })
        })?;
        let entry_point: TransactionEntryPoint =
            v1.deserialize_field(ENTRY_POINT_MAP_KEY).map_err(|error| {
                InvalidTransaction::V1(InvalidTransactionV1::CouldNotDeserializeField { error })
            })?;
        let scheduling: TransactionScheduling =
            v1.deserialize_field(SCHEDULING_MAP_KEY).map_err(|error| {
                InvalidTransaction::V1(InvalidTransactionV1::CouldNotDeserializeField { error })
            })?;

        if v1.number_of_fields() != EXPECTED_NUMBER_OF_FIELDS {
            return Err(InvalidTransaction::V1(
                InvalidTransactionV1::UnexpectedTransactionFieldEntries,
            ));
        }

        let payload_hash = v1.payload_hash()?;
        let serialized_length = v1.serialized_length();

        let lane_id = calculate_transaction_lane(
            &entry_point,
            &target,
            v1.pricing_mode().additional_computation_factor(),
            transaction_config,
            serialized_length as u64,
        )?;
        let transaction_lane =
            TransactionLane::try_from(lane_id).map_err(Into::<InvalidTransaction>::into)?;
        let has_valid_hash = v1.has_valid_hash();
        Ok(MetaTransactionV1::new(
            *v1.hash(),
            v1.chain_name().to_string(),
            v1.timestamp(),
            v1.ttl(),
            v1.pricing_mode().clone(),
            v1.initiator_addr().clone(),
            args,
            target,
            entry_point,
            transaction_lane,
            scheduling,
            serialized_length,
            payload_hash,
            v1.approvals().clone(),
            has_valid_hash,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        hash: TransactionV1Hash,
        chain_name: String,
        timestamp: Timestamp,
        ttl: TimeDiff,
        pricing_mode: PricingMode,
        initiator_addr: InitiatorAddr,
        args: RuntimeArgs,
        target: TransactionTarget,
        entry_point: TransactionEntryPoint,
        transaction_lane: TransactionLane,
        scheduling: TransactionScheduling,
        serialized_length: usize,
        payload_hash: Digest,
        approvals: BTreeSet<Approval>,
        has_valid_hash: Result<(), InvalidTransactionV1>,
    ) -> Self {
        Self {
            hash,
            chain_name,
            timestamp,
            ttl,
            pricing_mode,
            initiator_addr,
            args,
            target,
            entry_point,
            transaction_lane,
            scheduling,
            approvals,
            serialized_length,
            payload_hash,
            has_valid_hash,
            #[cfg(any(feature = "once_cell", test))]
            is_verified: OnceCell::new(),
        }
    }

    /// Returns the runtime args of the transaction.
    pub fn args(&self) -> &RuntimeArgs {
        &self.args
    }

    /// Returns the `DeployHash` identifying this `Deploy`.
    pub fn hash(&self) -> &TransactionV1Hash {
        &self.hash
    }

    /// Returns the `Approvals`.
    pub fn approvals(&self) -> &BTreeSet<Approval> {
        &self.approvals
    }

    /// Returns `Ok` if and only if:
    ///   * the transaction hash is correct (see [`TransactionV1::has_valid_hash`] for details)
    ///   * approvals are non empty, and
    ///   * all approvals are valid signatures of the signed hash
    pub fn verify(&self) -> Result<(), InvalidTransactionV1> {
        #[cfg(any(feature = "once_cell", test))]
        return self.is_verified.get_or_init(|| self.do_verify()).clone();

        #[cfg(not(any(feature = "once_cell", test)))]
        self.do_verify()
    }

    /// Returns `Ok` if and only if this transaction's body hashes to the value of `body_hash()`,
    /// and if this transaction's header hashes to the value claimed as the transaction hash.
    pub fn has_valid_hash(&self) -> &Result<(), InvalidTransactionV1> {
        &self.has_valid_hash
    }

    fn do_verify(&self) -> Result<(), InvalidTransactionV1> {
        if self.approvals.is_empty() {
            debug!(?self, "transaction has no approvals");
            return Err(InvalidTransactionV1::EmptyApprovals);
        }

        self.has_valid_hash().clone()?;

        for (index, approval) in self.approvals.iter().enumerate() {
            if let Err(error) = crypto::verify(self.hash, approval.signature(), approval.signer()) {
                debug!(
                    ?self,
                    "failed to verify transaction approval {}: {}", index, error
                );
                return Err(InvalidTransactionV1::InvalidApproval { index, error });
            }
        }

        Ok(())
    }

    /// Returns the entry point of the transaction.
    pub fn entry_point(&self) -> &TransactionEntryPoint {
        &self.entry_point
    }

    /// Returns the hash_addr and entry point name of a smart contract, if applicable.
    pub fn contract_direct_address(&self) -> Option<(HashAddr, String)> {
        let hash_addr = match self.target().contract_hash_addr() {
            Some(hash_addr) => hash_addr,
            None => return None,
        };
        let entry_point = match self.entry_point.custom_entry_point() {
            Some(entry_point) => entry_point,
            None => return None,
        };
        Some((hash_addr, entry_point))
    }

    /// Returns the transaction lane.
    pub fn transaction_lane(&self) -> u8 {
        self.transaction_lane as u8
    }

    /// Returns payload hash of the transaction.
    pub fn payload_hash(&self) -> &Digest {
        &self.payload_hash
    }

    /// Returns the pricing mode for the transaction.
    pub fn pricing_mode(&self) -> &PricingMode {
        &self.pricing_mode
    }

    /// Returns the initiator_addr of the transaction.
    pub fn initiator_addr(&self) -> &InitiatorAddr {
        &self.initiator_addr
    }

    /// Returns the target of the transaction.
    pub fn target(&self) -> &TransactionTarget {
        &self.target
    }

    /// Returns `true` if the serialized size of the transaction is not greater than
    /// `max_transaction_size`.
    fn is_valid_size(
        &self,
        max_transaction_size: u32,
    ) -> Result<(), TransactionV1ExcessiveSizeError> {
        let actual_transaction_size = self.serialized_length;
        if actual_transaction_size > max_transaction_size as usize {
            return Err(TransactionV1ExcessiveSizeError {
                max_transaction_size,
                actual_transaction_size,
            });
        }
        Ok(())
    }

    /// Returns the creation timestamp of the `Deploy`.
    pub fn timestamp(&self) -> Timestamp {
        self.timestamp
    }

    /// Returns the duration after the creation timestamp for which the `Deploy` will stay valid.
    ///
    /// After this duration has ended, the `Deploy` will be considered expired.
    pub fn ttl(&self) -> TimeDiff {
        self.ttl
    }

    /// Returns `Ok` if and only if:
    ///   * the chain_name is correct,
    ///   * the configured parameters are complied with at the given timestamp
    pub fn is_config_compliant(
        &self,
        chainspec: &Chainspec,
        timestamp_leeway: TimeDiff,
        at: Timestamp,
    ) -> Result<(), InvalidTransactionV1> {
        let transaction_config = chainspec.transaction_config.clone();
        self.is_valid_size(
            transaction_config
                .transaction_v1_config
                .get_max_serialized_length(self.transaction_lane as u8) as u32,
        )?;

        let chain_name = chainspec.network_config.name.clone();

        if self.chain_name != chain_name {
            debug!(
                transaction_hash = %self.hash(),
                chain_name = %self.chain_name,
                timestamp= %self.timestamp,
                ttl= %self.ttl,
                pricing_mode= %self.pricing_mode,
                initiator_addr= %self.initiator_addr,
                target= %self.target,
                entry_point= %self.entry_point,
                transaction_lane= %self.transaction_lane,
                scheduling= %self.scheduling,
                "invalid chain identifier"
            );
            return Err(InvalidTransactionV1::InvalidChainName {
                expected: chain_name,
                got: self.chain_name.to_string(),
            });
        }

        let price_handling = chainspec.core_config.pricing_handling;
        let pricing_mode = &self.pricing_mode;

        match pricing_mode {
            PricingMode::Classic { .. } => {
                if let PricingHandling::Classic = price_handling {
                } else {
                    return Err(InvalidTransactionV1::InvalidPricingMode {
                        price_mode: pricing_mode.clone(),
                    });
                }
            }
            PricingMode::Fixed { .. } => {
                if let PricingHandling::Fixed = price_handling {
                } else {
                    return Err(InvalidTransactionV1::InvalidPricingMode {
                        price_mode: pricing_mode.clone(),
                    });
                }
            }
            PricingMode::Reserved { .. } => {
                if !chainspec.core_config.allow_reservations {
                    // Currently Reserved isn't implemented and we should
                    // not be accepting transactions with this mode.
                    return Err(InvalidTransactionV1::InvalidPricingMode {
                        price_mode: pricing_mode.clone(),
                    });
                }
            }
        }

        let min_gas_price = chainspec.vacancy_config.min_gas_price;
        let gas_price_tolerance = self.gas_price_tolerance();
        if gas_price_tolerance < min_gas_price {
            return Err(InvalidTransactionV1::GasPriceToleranceTooLow {
                min_gas_price_tolerance: min_gas_price,
                provided_gas_price_tolerance: gas_price_tolerance,
            });
        }

        self.is_header_metadata_valid(&transaction_config, timestamp_leeway, at, &self.hash)?;

        let max_associated_keys = chainspec.core_config.max_associated_keys;

        if self.approvals.len() > max_associated_keys as usize {
            debug!(
                transaction_hash = %self.hash(),
                number_of_approvals = %self.approvals.len(),
                max_associated_keys = %max_associated_keys,
                "number of transaction approvals exceeds the limit"
            );
            return Err(InvalidTransactionV1::ExcessiveApprovals {
                got: self.approvals.len() as u32,
                max_associated_keys,
            });
        }

        let gas_limit = self
            .pricing_mode
            .gas_limit(chainspec, &self.entry_point, self.transaction_lane as u8)
            .map_err(Into::<InvalidTransactionV1>::into)?;
        let block_gas_limit = Gas::new(U512::from(transaction_config.block_gas_limit));
        if gas_limit > block_gas_limit {
            debug!(
                amount = %gas_limit,
                %block_gas_limit,
                "transaction gas limit exceeds block gas limit"
            );
            return Err(InvalidTransactionV1::ExceedsBlockGasLimit {
                block_gas_limit: transaction_config.block_gas_limit,
                got: Box::new(gas_limit.value()),
            });
        }

        self.is_body_metadata_valid(&transaction_config)
    }

    fn is_body_metadata_valid(
        &self,
        config: &TransactionConfig,
    ) -> Result<(), InvalidTransactionV1> {
        let lane_id = self.transaction_lane as u8;
        if !config.transaction_v1_config.is_supported(lane_id) {
            return Err(InvalidTransactionV1::InvalidTransactionLane(lane_id));
        }

        let max_serialized_length = config
            .transaction_v1_config
            .get_max_serialized_length(lane_id);
        let actual_length = self.serialized_length;
        if actual_length > max_serialized_length as usize {
            return Err(InvalidTransactionV1::ExcessiveSize(
                TransactionV1ExcessiveSizeError {
                    max_transaction_size: max_serialized_length as u32,
                    actual_transaction_size: actual_length,
                },
            ));
        }

        let max_args_length = config.transaction_v1_config.get_max_args_length(lane_id);

        let args_length = self.args.serialized_length();
        if args_length > max_args_length as usize {
            debug!(
                args_length,
                max_args_length = max_args_length,
                "transaction runtime args excessive size"
            );
            return Err(InvalidTransactionV1::ExcessiveArgsLength {
                max_length: max_args_length as usize,
                got: args_length,
            });
        }

        match &self.target {
            TransactionTarget::Native => match self.entry_point {
                TransactionEntryPoint::Call => {
                    debug!(
                        entry_point = %self.entry_point,
                        "native transaction cannot have call entry point"
                    );
                    Err(InvalidTransactionV1::EntryPointCannotBeCall)
                }
                TransactionEntryPoint::Custom(_) => {
                    debug!(
                        entry_point = %self.entry_point,
                        "native transaction cannot have custom entry point"
                    );
                    Err(InvalidTransactionV1::EntryPointCannotBeCustom {
                        entry_point: self.entry_point.clone(),
                    })
                }
                TransactionEntryPoint::Transfer => arg_handling::has_valid_transfer_args(
                    &self.args,
                    config.native_transfer_minimum_motes,
                ),
                TransactionEntryPoint::AddBid => arg_handling::has_valid_add_bid_args(&self.args),
                TransactionEntryPoint::WithdrawBid => {
                    arg_handling::has_valid_withdraw_bid_args(&self.args)
                }
                TransactionEntryPoint::Delegate => {
                    arg_handling::has_valid_delegate_args(&self.args)
                }
                TransactionEntryPoint::Undelegate => {
                    arg_handling::has_valid_undelegate_args(&self.args)
                }
                TransactionEntryPoint::Redelegate => {
                    arg_handling::has_valid_redelegate_args(&self.args)
                }
                TransactionEntryPoint::ActivateBid => {
                    arg_handling::has_valid_activate_bid_args(&self.args)
                }
                TransactionEntryPoint::ChangeBidPublicKey => {
                    arg_handling::has_valid_change_bid_public_key_args(&self.args)
                }
                TransactionEntryPoint::AddReservations => {
                    arg_handling::has_valid_add_reservations_args(&self.args)
                }
                TransactionEntryPoint::CancelReservations => {
                    arg_handling::has_valid_cancel_reservations_args(&self.args)
                }
            },
            TransactionTarget::Stored { .. } => match &self.entry_point {
                TransactionEntryPoint::Custom(_) => Ok(()),
                TransactionEntryPoint::Call
                | TransactionEntryPoint::Transfer
                | TransactionEntryPoint::AddBid
                | TransactionEntryPoint::WithdrawBid
                | TransactionEntryPoint::Delegate
                | TransactionEntryPoint::Undelegate
                | TransactionEntryPoint::Redelegate
                | TransactionEntryPoint::ActivateBid
                | TransactionEntryPoint::ChangeBidPublicKey
                | TransactionEntryPoint::AddReservations
                | TransactionEntryPoint::CancelReservations => {
                    debug!(
                        entry_point = %self.entry_point,
                        "transaction targeting stored entity/package must have custom entry point"
                    );
                    Err(InvalidTransactionV1::EntryPointMustBeCustom {
                        entry_point: self.entry_point.clone(),
                    })
                }
            },
            TransactionTarget::Session { module_bytes, .. } => match &self.entry_point {
                TransactionEntryPoint::Call | TransactionEntryPoint::Custom(_) => {
                    if module_bytes.is_empty() {
                        debug!("transaction with session code must not have empty module bytes");
                        return Err(InvalidTransactionV1::EmptyModuleBytes);
                    }
                    Ok(())
                }
                TransactionEntryPoint::Transfer
                | TransactionEntryPoint::AddBid
                | TransactionEntryPoint::WithdrawBid
                | TransactionEntryPoint::Delegate
                | TransactionEntryPoint::Undelegate
                | TransactionEntryPoint::Redelegate
                | TransactionEntryPoint::ActivateBid
                | TransactionEntryPoint::ChangeBidPublicKey
                | TransactionEntryPoint::AddReservations
                | TransactionEntryPoint::CancelReservations => {
                    debug!(
                        entry_point = %self.entry_point,
                        "transaction with session code must use custom or default 'call' entry point"
                    );
                    Err(InvalidTransactionV1::EntryPointMustBeCustom {
                        entry_point: self.entry_point.clone(),
                    })
                }
            },
        }
    }

    fn is_header_metadata_valid(
        &self,
        config: &TransactionConfig,
        timestamp_leeway: TimeDiff,
        at: Timestamp,
        transaction_hash: &TransactionV1Hash,
    ) -> Result<(), InvalidTransactionV1> {
        if self.ttl() > config.max_ttl {
            debug!(
                %transaction_hash,
                transaction_header = %self,
                max_ttl = %config.max_ttl,
                "transaction ttl excessive"
            );
            return Err(InvalidTransactionV1::ExcessiveTimeToLive {
                max_ttl: config.max_ttl,
                got: self.ttl(),
            });
        }

        if self.timestamp() > at + timestamp_leeway {
            debug!(
                %transaction_hash, transaction_header = %self, %at,
                "transaction timestamp in the future"
            );
            return Err(InvalidTransactionV1::TimestampInFuture {
                validation_timestamp: at,
                timestamp_leeway,
                got: self.timestamp(),
            });
        }

        Ok(())
    }

    /// Returns the gas price tolerance for the given transaction.
    pub fn gas_price_tolerance(&self) -> u8 {
        match self.pricing_mode {
            PricingMode::Classic {
                gas_price_tolerance,
                ..
            } => gas_price_tolerance,
            PricingMode::Fixed {
                gas_price_tolerance,
                ..
            } => gas_price_tolerance,
            PricingMode::Reserved { .. } => {
                // TODO: Change this when reserve gets implemented.
                0u8
            }
        }
    }

    pub fn serialized_length(&self) -> usize {
        self.serialized_length
    }

    pub fn gas_limit(&self, chainspec: &Chainspec) -> Result<Gas, InvalidTransaction> {
        self.pricing_mode()
            .gas_limit(chainspec, self.entry_point(), self.transaction_lane as u8)
            .map_err(Into::into)
    }
}

impl Display for MetaTransactionV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        write!(
            formatter,
            "meta-transaction-v1[hash: {}, chain_name: {}, timestamp: {}, ttl: {}, pricing_mode: {}, initiator_addr: {}, target: {}, entry_point: {}, transaction_lane: {}, scheduling: {}, approvals: {}]",
            self.hash,
            self.chain_name,
            self.timestamp,
            self.ttl,
            self.pricing_mode,
            self.initiator_addr,
            self.target,
            self.entry_point,
            self.transaction_lane,
            self.scheduling,
            DisplayIter::new(self.approvals.iter())
        )
    }
}
