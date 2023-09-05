use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::AuctionTransactionV1;
#[cfg(doc)]
use super::TransactionV1;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    RuntimeArgs,
};

const MINT_TRANSFER_TAG: u8 = 0;
const AUCTION_TAG: u8 = 1;
const RESERVATION_TAG: u8 = 2;

/// A [`TransactionV1`] targeting native functionality.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "A TransactionV1 targeting native functionality.")
)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub enum NativeTransactionV1 {
    /// Calls the `transfer` entry point of the mint to transfer `Motes` from a source purse to a
    /// target purse.
    ///
    /// Requires the following runtime args:
    ///   * "source": `URef`
    ///   * "target": `URef`
    ///   * "amount": `U512`
    ///
    /// The following optional runtime args can also be provided:
    ///   * "to": `Option<AccountHash>`
    ///   * "id": `Option<u64>`
    #[cfg_attr(
        feature = "json-schema",
        schemars(
            description = "Calls the `transfer` entry point of the mint to transfer `Motes` from
            a source purse to a target purse."
        )
    )]
    MintTransfer(RuntimeArgs),

    /// A transaction targeting the auction.
    Auction(AuctionTransactionV1),

    /// A transaction reserving a future execution.
    Reservation(RuntimeArgs),
}

impl NativeTransactionV1 {
    /// Returns a new `NativeTransactionV1::MintTransfer`.
    pub fn new_mint_transfer(args: RuntimeArgs) -> Self {
        NativeTransactionV1::MintTransfer(args)
    }

    /// Returns a new `NativeTransactionV1::Auction`.
    pub fn new_auction(auction_transaction: AuctionTransactionV1) -> Self {
        NativeTransactionV1::Auction(auction_transaction)
    }

    /// Returns a new `NativeTransactionV1::Reservation`.
    pub fn new_reservation(args: RuntimeArgs) -> Self {
        NativeTransactionV1::Reservation(args)
    }

    /// Returns the runtime arguments.
    pub fn args(&self) -> &RuntimeArgs {
        match self {
            NativeTransactionV1::MintTransfer(args) | NativeTransactionV1::Reservation(args) => {
                args
            }
            NativeTransactionV1::Auction(auction_transaction) => auction_transaction.args(),
        }
    }

    /// Returns a random `NativeTransactionV1`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            0 => NativeTransactionV1::MintTransfer(RuntimeArgs::random(rng)),
            1 => NativeTransactionV1::Auction(AuctionTransactionV1::random(rng)),
            2 => NativeTransactionV1::Reservation(RuntimeArgs::random(rng)),
            _ => unreachable!(),
        }
    }
}

impl Display for NativeTransactionV1 {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            NativeTransactionV1::MintTransfer(_) => write!(formatter, "mint transfer"),
            NativeTransactionV1::Auction(auction_txn) => {
                write!(formatter, "native: {}", auction_txn)
            }
            NativeTransactionV1::Reservation(_) => write!(formatter, "reservation"),
        }
    }
}

impl ToBytes for NativeTransactionV1 {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            NativeTransactionV1::MintTransfer(args) => {
                MINT_TRANSFER_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            NativeTransactionV1::Auction(auction_txn) => {
                AUCTION_TAG.write_bytes(writer)?;
                auction_txn.write_bytes(writer)
            }
            NativeTransactionV1::Reservation(args) => {
                RESERVATION_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
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
                NativeTransactionV1::MintTransfer(args) => args.serialized_length(),
                NativeTransactionV1::Auction(auction_txn) => auction_txn.serialized_length(),
                NativeTransactionV1::Reservation(args) => args.serialized_length(),
            }
    }
}

impl FromBytes for NativeTransactionV1 {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            MINT_TRANSFER_TAG => {
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((NativeTransactionV1::MintTransfer(args), remainder))
            }
            AUCTION_TAG => {
                let (auction_txn, remainder) = AuctionTransactionV1::from_bytes(remainder)?;
                Ok((NativeTransactionV1::Auction(auction_txn), remainder))
            }
            RESERVATION_TAG => {
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((NativeTransactionV1::Reservation(args), remainder))
            }
            _ => Err(bytesrepr::Error::Formatting),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();
        for _ in 0..10 {
            bytesrepr::test_serialization_roundtrip(&NativeTransactionV1::random(rng));
        }
    }
}
