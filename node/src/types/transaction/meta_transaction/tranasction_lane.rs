use core::{
    convert::TryFrom,
    fmt::{self, Formatter},
};

use casper_types::{
    InvalidTransaction, InvalidTransactionV1, TransactionConfig, TransactionEntryPoint,
    TransactionTarget, TransactionV1Config, AUCTION_LANE_ID, INSTALL_UPGRADE_LANE_ID, MINT_LANE_ID,
};
#[cfg(feature = "datasize")]
use datasize::DataSize;
use serde::Serialize;

/// The category of a Transaction.
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Serialize)]
#[repr(u8)]
pub enum TransactionLane {
    /// Native mint interaction (the default).
    Mint = 0,
    /// Native auction interaction.
    Auction = 1,
    /// InstallUpgradeWasm
    InstallUpgradeWasm = 2,
    /// A large Wasm based transaction.
    Large = 3,
    /// A medium Wasm based transaction.
    Medium = 4,
    /// A small Wasm based transaction.
    Small = 5,
}

#[derive(Debug)]
pub struct TransactionLaneConversionError(u8);

impl From<TransactionLaneConversionError> for InvalidTransaction {
    fn from(value: TransactionLaneConversionError) -> InvalidTransaction {
        InvalidTransaction::V1(InvalidTransactionV1::InvalidTransactionLane(value.0))
    }
}

impl TryFrom<u8> for TransactionLane {
    type Error = TransactionLaneConversionError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Mint),
            1 => Ok(Self::Auction),
            2 => Ok(Self::InstallUpgradeWasm),
            3 => Ok(Self::Large),
            4 => Ok(Self::Medium),
            5 => Ok(Self::Small),
            _ => Err(TransactionLaneConversionError(value)),
        }
    }
}

impl fmt::Display for TransactionLane {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionLane::Mint => write!(f, "Mint"),
            TransactionLane::Auction => write!(f, "Auction"),
            TransactionLane::Large => write!(f, "Large"),
            TransactionLane::Medium => write!(f, "Medium"),
            TransactionLane::Small => write!(f, "Small"),
            TransactionLane::InstallUpgradeWasm => write!(f, "InstallUpgradeWASM"),
        }
    }
}

/// Calculates the laned based on properties of the transaction
pub(crate) fn calculate_transaction_lane(
    entry_point: &TransactionEntryPoint,
    target: &TransactionTarget,
    additional_computation_factor: u8,
    transaction_config: &TransactionConfig,
    transaction_size: u64,
) -> Result<u8, InvalidTransactionV1> {
    match target {
        TransactionTarget::Native => match entry_point {
            TransactionEntryPoint::Transfer => Ok(MINT_LANE_ID),
            TransactionEntryPoint::AddBid
            | TransactionEntryPoint::WithdrawBid
            | TransactionEntryPoint::Delegate
            | TransactionEntryPoint::Undelegate
            | TransactionEntryPoint::Redelegate
            | TransactionEntryPoint::ActivateBid
            | TransactionEntryPoint::ChangeBidPublicKey
            | TransactionEntryPoint::AddReservations
            | TransactionEntryPoint::CancelReservations => Ok(AUCTION_LANE_ID),
            TransactionEntryPoint::Call => Err(InvalidTransactionV1::EntryPointCannotBeCall),
            TransactionEntryPoint::Custom(_) => {
                Err(InvalidTransactionV1::EntryPointCannotBeCustom {
                    entry_point: entry_point.clone(),
                })
            }
        },
        TransactionTarget::Stored { .. } => match entry_point {
            TransactionEntryPoint::Custom(_) => get_lane_for_non_install_wasm(
                &transaction_config.transaction_v1_config,
                transaction_size,
                additional_computation_factor,
            ),
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
                Err(InvalidTransactionV1::EntryPointMustBeCustom {
                    entry_point: entry_point.clone(),
                })
            }
        },
        TransactionTarget::Session {
            is_install_upgrade, ..
        } => match entry_point {
            TransactionEntryPoint::Call => {
                if *is_install_upgrade {
                    Ok(INSTALL_UPGRADE_LANE_ID)
                } else {
                    get_lane_for_non_install_wasm(
                        &transaction_config.transaction_v1_config,
                        transaction_size,
                        additional_computation_factor,
                    )
                }
            }
            TransactionEntryPoint::Custom(_)
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
                Err(InvalidTransactionV1::EntryPointMustBeCall {
                    entry_point: entry_point.clone(),
                })
            }
        },
    }
}

fn get_lane_for_non_install_wasm(
    config: &TransactionV1Config,
    transaction_size: u64,
    additional_computation_factor: u8,
) -> Result<u8, InvalidTransactionV1> {
    config
        .get_wasm_lane_id(transaction_size, additional_computation_factor)
        .ok_or(InvalidTransactionV1::NoWasmLaneMatchesTransaction())
}
