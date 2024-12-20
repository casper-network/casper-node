use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(doc)]
use super::Transaction;
use super::{
    serialization::CalltableSerializationEnvelope, InvalidTransaction, InvalidTransactionV1,
    TransactionEntryPoint,
};
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{
        Error::{self, Formatting},
        FromBytes, ToBytes,
    },
    transaction::serialization::CalltableSerializationEnvelopeBuilder,
    Digest,
};
#[cfg(any(feature = "std", test))]
use crate::{Chainspec, Gas, Motes, AUCTION_LANE_ID, MINT_LANE_ID, U512};

/// The pricing mode of a [`Transaction`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "Pricing mode of a Transaction.")
)]
#[serde(deny_unknown_fields)]
pub enum PricingMode {
    /// The original payment model, where the creator of the transaction
    /// specifies how much they will pay, at what gas price.
    PaymentLimited {
        /// User-specified payment amount.
        payment_amount: u64,
        /// User-specified gas_price tolerance (minimum 1).
        /// This is interpreted to mean "do not include this transaction in a block
        /// if the current gas price is greater than this number"
        gas_price_tolerance: u8,
        /// Standard payment.
        standard_payment: bool,
    },
    /// The cost of the transaction is determined by the cost table, per the
    /// transaction category.
    Fixed {
        /// User-specified additional computation factor (minimum 0). If "0" is provided,
        ///  no additional logic is applied to the computation limit. Each value above "0"
        ///  tells the node that it needs to treat the transaction as if it uses more gas
        ///  than it's serialized size indicates. Each "1" will increase the "wasm lane"
        ///  size bucket for this transaction by 1. So if the size of the transaction
        ///  indicates bucket "0" and "additional_computation_factor = 2", the transaction
        ///  will be treated as a "2".
        additional_computation_factor: u8,
        /// User-specified gas_price tolerance (minimum 1).
        /// This is interpreted to mean "do not include this transaction in a block
        /// if the current gas price is greater than this number"
        gas_price_tolerance: u8,
    },
    /// The payment for this transaction was previously paid, as proven by
    /// the receipt hash (this is for future use, not currently implemented).
    Prepaid {
        /// Pre-paid receipt.
        receipt: Digest,
    },
}

impl PricingMode {
    /// Returns a random `PricingMode.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..=2) {
            0 => PricingMode::PaymentLimited {
                payment_amount: rng.gen(),
                gas_price_tolerance: 1,
                standard_payment: true,
            },
            1 => PricingMode::Fixed {
                gas_price_tolerance: rng.gen(),
                additional_computation_factor: 1,
            },
            2 => PricingMode::Prepaid { receipt: rng.gen() },
            _ => unreachable!(),
        }
    }

    /// Returns standard payment flag, if it is a `PaymentLimited` variant.
    pub fn is_standard_payment(&self) -> bool {
        match self {
            PricingMode::PaymentLimited {
                standard_payment, ..
            } => *standard_payment,
            PricingMode::Fixed { .. } => true,
            PricingMode::Prepaid { .. } => true,
        }
    }

    fn serialized_field_lengths(&self) -> Vec<usize> {
        match self {
            PricingMode::PaymentLimited {
                payment_amount,
                gas_price_tolerance,
                standard_payment,
            } => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    payment_amount.serialized_length(),
                    gas_price_tolerance.serialized_length(),
                    standard_payment.serialized_length(),
                ]
            }
            PricingMode::Fixed {
                gas_price_tolerance,
                additional_computation_factor,
            } => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    gas_price_tolerance.serialized_length(),
                    additional_computation_factor.serialized_length(),
                ]
            }
            PricingMode::Prepaid { receipt } => {
                vec![
                    crate::bytesrepr::U8_SERIALIZED_LENGTH,
                    receipt.serialized_length(),
                ]
            }
        }
    }

    #[cfg(any(feature = "std", test))]
    /// Returns the gas limit.
    pub fn gas_limit(
        &self,
        chainspec: &Chainspec,
        entry_point: &TransactionEntryPoint,
        lane_id: u8,
    ) -> Result<Gas, PricingModeError> {
        let costs = chainspec.system_costs_config;
        let gas = match self {
            PricingMode::PaymentLimited { payment_amount, .. } => Gas::new(*payment_amount),
            PricingMode::Fixed { .. } => {
                let computation_limit = {
                    if lane_id == MINT_LANE_ID {
                        // Because we currently only support one native mint interaction,
                        // native transfer, we can short circuit to return that value.
                        // However if other direct mint interactions are supported
                        // in the future (such as the upcoming burn feature),
                        // this logic will need to be expanded to self.mint_costs().field?
                        // for the value for each verb...see how auction is set up below.
                        costs.mint_costs().transfer as u64
                    } else if lane_id == AUCTION_LANE_ID {
                        let amount = match entry_point {
                            TransactionEntryPoint::Call => {
                                return Err(PricingModeError::EntryPointCannotBeCall)
                            }
                            TransactionEntryPoint::Custom(_) | TransactionEntryPoint::Transfer => {
                                return Err(PricingModeError::EntryPointCannotBeCustom {
                                    entry_point: entry_point.clone(),
                                });
                            }
                            TransactionEntryPoint::AddBid | TransactionEntryPoint::ActivateBid => {
                                costs.auction_costs().add_bid
                            }
                            TransactionEntryPoint::WithdrawBid => {
                                costs.auction_costs().withdraw_bid
                            }
                            TransactionEntryPoint::Delegate => costs.auction_costs().delegate,
                            TransactionEntryPoint::Undelegate => costs.auction_costs().undelegate,
                            TransactionEntryPoint::Redelegate => costs.auction_costs().redelegate,
                            TransactionEntryPoint::ChangeBidPublicKey => {
                                costs.auction_costs().change_bid_public_key
                            }
                            TransactionEntryPoint::AddReservations => {
                                costs.auction_costs().add_reservations
                            }
                            TransactionEntryPoint::CancelReservations => {
                                costs.auction_costs().cancel_reservations
                            }
                        };
                        amount
                    } else {
                        chainspec.get_max_gas_limit_by_category(lane_id)
                    }
                };
                Gas::new(U512::from(computation_limit))
            }
            PricingMode::Prepaid { receipt } => {
                return Err(PricingModeError::InvalidPricingMode {
                    price_mode: PricingMode::Prepaid { receipt: *receipt },
                });
            }
        };
        Ok(gas)
    }

    #[cfg(any(feature = "std", test))]
    /// Returns gas cost.
    pub fn gas_cost(
        &self,
        chainspec: &Chainspec,
        entry_point: &TransactionEntryPoint,
        lane_id: u8,
        gas_price: u8,
    ) -> Result<Motes, PricingModeError> {
        let gas_limit = self.gas_limit(chainspec, entry_point, lane_id)?;
        let motes = match self {
            PricingMode::PaymentLimited { .. } | PricingMode::Fixed { .. } => {
                Motes::from_gas(gas_limit, gas_price)
                    .ok_or(PricingModeError::UnableToCalculateGasCost)?
            }
            PricingMode::Prepaid { .. } => {
                Motes::zero() // prepaid
            }
        };
        Ok(motes)
    }

    /// Returns gas cost.
    pub fn additional_computation_factor(&self) -> u8 {
        match self {
            PricingMode::PaymentLimited { .. } => 0,
            PricingMode::Fixed {
                additional_computation_factor,
                ..
            } => *additional_computation_factor,
            PricingMode::Prepaid { .. } => 0,
        }
    }
}

///Errors that can occur when calling PricingMode functions
pub enum PricingModeError {
    /// The entry point for this transaction target cannot be `call`.
    EntryPointCannotBeCall,
    /// The entry point for this transaction target cannot be `TransactionEntryPoint::Custom`.
    EntryPointCannotBeCustom {
        /// The invalid entry point.
        entry_point: TransactionEntryPoint,
    },
    /// Invalid combination of pricing handling and pricing mode.
    InvalidPricingMode {
        /// The pricing mode as specified by the transaction.
        price_mode: PricingMode,
    },
    /// Unable to calculate gas cost.
    UnableToCalculateGasCost,
}

impl From<PricingModeError> for InvalidTransaction {
    fn from(err: PricingModeError) -> Self {
        InvalidTransaction::V1(err.into())
    }
}

impl From<PricingModeError> for InvalidTransactionV1 {
    fn from(err: PricingModeError) -> Self {
        match err {
            PricingModeError::EntryPointCannotBeCall => {
                InvalidTransactionV1::EntryPointCannotBeCall
            }
            PricingModeError::EntryPointCannotBeCustom { entry_point } => {
                InvalidTransactionV1::EntryPointCannotBeCustom { entry_point }
            }
            PricingModeError::InvalidPricingMode { price_mode } => {
                InvalidTransactionV1::InvalidPricingMode { price_mode }
            }
            PricingModeError::UnableToCalculateGasCost => {
                InvalidTransactionV1::UnableToCalculateGasCost
            }
        }
    }
}
const TAG_FIELD_INDEX: u16 = 0;

const PAYMENT_LIMITED_VARIANT_TAG: u8 = 0;
const PAYMENT_LIMITED_PAYMENT_AMOUNT_INDEX: u16 = 1;
const PAYMENT_LIMITED_GAS_PRICE_TOLERANCE_INDEX: u16 = 2;
const PAYMENT_LIMITED_STANDARD_PAYMENT_INDEX: u16 = 3;

const FIXED_VARIANT_TAG: u8 = 1;
const FIXED_GAS_PRICE_TOLERANCE_INDEX: u16 = 1;
const FIXED_ADDITIONAL_COMPUTATION_FACTOR_INDEX: u16 = 2;

const RESERVED_VARIANT_TAG: u8 = 2;
const RESERVED_RECEIPT_INDEX: u16 = 1;

impl ToBytes for PricingMode {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        match self {
            PricingMode::PaymentLimited {
                payment_amount,
                gas_price_tolerance,
                standard_payment,
            } => CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                .add_field(TAG_FIELD_INDEX, &PAYMENT_LIMITED_VARIANT_TAG)?
                .add_field(PAYMENT_LIMITED_PAYMENT_AMOUNT_INDEX, &payment_amount)?
                .add_field(
                    PAYMENT_LIMITED_GAS_PRICE_TOLERANCE_INDEX,
                    &gas_price_tolerance,
                )?
                .add_field(PAYMENT_LIMITED_STANDARD_PAYMENT_INDEX, &standard_payment)?
                .binary_payload_bytes(),
            PricingMode::Fixed {
                gas_price_tolerance,
                additional_computation_factor,
            } => CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                .add_field(TAG_FIELD_INDEX, &FIXED_VARIANT_TAG)?
                .add_field(FIXED_GAS_PRICE_TOLERANCE_INDEX, &gas_price_tolerance)?
                .add_field(
                    FIXED_ADDITIONAL_COMPUTATION_FACTOR_INDEX,
                    &additional_computation_factor,
                )?
                .binary_payload_bytes(),
            PricingMode::Prepaid { receipt } => {
                CalltableSerializationEnvelopeBuilder::new(self.serialized_field_lengths())?
                    .add_field(TAG_FIELD_INDEX, &RESERVED_VARIANT_TAG)?
                    .add_field(RESERVED_RECEIPT_INDEX, &receipt)?
                    .binary_payload_bytes()
            }
        }
    }
    fn serialized_length(&self) -> usize {
        CalltableSerializationEnvelope::estimate_size(self.serialized_field_lengths())
    }
}

impl FromBytes for PricingMode {
    fn from_bytes(bytes: &[u8]) -> Result<(PricingMode, &[u8]), Error> {
        let (binary_payload, remainder) = CalltableSerializationEnvelope::from_bytes(4, bytes)?;
        let window = binary_payload.start_consuming()?.ok_or(Formatting)?;
        window.verify_index(TAG_FIELD_INDEX)?;
        let (tag, window) = window.deserialize_and_maybe_next::<u8>()?;
        let to_ret = match tag {
            PAYMENT_LIMITED_VARIANT_TAG => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(PAYMENT_LIMITED_PAYMENT_AMOUNT_INDEX)?;
                let (payment_amount, window) = window.deserialize_and_maybe_next::<u64>()?;
                let window = window.ok_or(Formatting)?;
                window.verify_index(PAYMENT_LIMITED_GAS_PRICE_TOLERANCE_INDEX)?;
                let (gas_price_tolerance, window) = window.deserialize_and_maybe_next::<u8>()?;
                let window = window.ok_or(Formatting)?;
                window.verify_index(PAYMENT_LIMITED_STANDARD_PAYMENT_INDEX)?;
                let (standard_payment, window) = window.deserialize_and_maybe_next::<bool>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(PricingMode::PaymentLimited {
                    payment_amount,
                    gas_price_tolerance,
                    standard_payment,
                })
            }
            FIXED_VARIANT_TAG => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(FIXED_GAS_PRICE_TOLERANCE_INDEX)?;
                let (gas_price_tolerance, window) = window.deserialize_and_maybe_next::<u8>()?;
                let window = window.ok_or(Formatting)?;
                window.verify_index(FIXED_ADDITIONAL_COMPUTATION_FACTOR_INDEX)?;
                let (additional_computation_factor, window) =
                    window.deserialize_and_maybe_next::<u8>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(PricingMode::Fixed {
                    gas_price_tolerance,
                    additional_computation_factor,
                })
            }
            RESERVED_VARIANT_TAG => {
                let window = window.ok_or(Formatting)?;
                window.verify_index(RESERVED_RECEIPT_INDEX)?;
                let (receipt, window) = window.deserialize_and_maybe_next::<Digest>()?;
                if window.is_some() {
                    return Err(Formatting);
                }
                Ok(PricingMode::Prepaid { receipt })
            }
            _ => Err(Formatting),
        };
        to_ret.map(|endpoint| (endpoint, remainder))
    }
}

impl Display for PricingMode {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            PricingMode::PaymentLimited {
                payment_amount,
                gas_price_tolerance: gas_price,
                standard_payment,
            } => {
                write!(
                    formatter,
                    "payment amount {}, gas price multiplier {} standard_payment {}",
                    payment_amount, gas_price, standard_payment
                )
            }
            PricingMode::Prepaid { receipt } => write!(formatter, "prepaid: {}", receipt),
            PricingMode::Fixed {
                gas_price_tolerance,
                additional_computation_factor,
            } => write!(
                formatter,
                "fixed pricing {} {}",
                gas_price_tolerance, additional_computation_factor
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytesrepr;

    #[test]
    fn test_to_bytes_and_from_bytes() {
        let classic = PricingMode::PaymentLimited {
            payment_amount: 100,
            gas_price_tolerance: 1,
            standard_payment: true,
        };
        match classic {
            PricingMode::PaymentLimited { .. } => {}
            PricingMode::Fixed { .. } => {}
            PricingMode::Prepaid { .. } => {}
        }
        bytesrepr::test_serialization_roundtrip(&classic);
        bytesrepr::test_serialization_roundtrip(&PricingMode::Fixed {
            gas_price_tolerance: 2,
            additional_computation_factor: 1,
        });
        bytesrepr::test_serialization_roundtrip(&PricingMode::Prepaid {
            receipt: Digest::hash(b"prepaid"),
        });
    }

    use crate::gens::pricing_mode_arb;
    use proptest::prelude::*;
    proptest! {
        #[test]
        fn generative_bytesrepr_roundtrip(val in pricing_mode_arb()) {
            bytesrepr::test_serialization_roundtrip(&val);
        }
    }
}
