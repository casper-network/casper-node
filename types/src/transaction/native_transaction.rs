use alloc::vec::Vec;
use core::fmt::{self, Display, Formatter};

#[cfg(feature = "datasize")]
use datasize::DataSize;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
#[cfg(feature = "json-schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::AuctionTransaction;
#[cfg(doc)]
use super::Transaction;
#[cfg(any(feature = "testing", test))]
use crate::testing::TestRng;
use crate::{
    bytesrepr::{self, FromBytes, ToBytes, U8_SERIALIZED_LENGTH},
    RuntimeArgs,
};

const MINT_TRANSFER_TAG: u8 = 0;
const AUCTION_TAG: u8 = 1;
const RESERVATION_TAG: u8 = 2;

/// A [`Transaction`] targeting native functionality.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Serialize, Deserialize, Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
#[cfg_attr(
    feature = "json-schema",
    derive(JsonSchema),
    schemars(description = "A Transaction targeting native functionality.")
)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub enum NativeTransaction {
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
    Auction(AuctionTransaction),

    /// A transaction reserving a future execution.
    Reservation(RuntimeArgs),
}

impl NativeTransaction {
    /// Returns a new `NativeTransaction::MintTransfer`.
    pub fn new_mint_transfer(args: RuntimeArgs) -> Self {
        NativeTransaction::MintTransfer(args)
    }

    /// Returns a new `NativeTransaction::Auction`.
    pub fn new_auction(auction_transaction: AuctionTransaction) -> Self {
        NativeTransaction::Auction(auction_transaction)
    }

    /// Returns a new `NativeTransaction::Reservation`.
    pub fn new_reservation(args: RuntimeArgs) -> Self {
        NativeTransaction::Reservation(args)
    }

    /// Returns the runtime arguments.
    pub fn args(&self) -> &RuntimeArgs {
        match self {
            NativeTransaction::MintTransfer(args) | NativeTransaction::Reservation(args) => args,
            NativeTransaction::Auction(auction_transaction) => auction_transaction.args(),
        }
    }

    /// Returns a random `NativeTransaction`.
    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        match rng.gen_range(0..3) {
            0 => NativeTransaction::MintTransfer(RuntimeArgs::random(rng)),
            1 => NativeTransaction::Auction(AuctionTransaction::random(rng)),
            2 => NativeTransaction::Reservation(RuntimeArgs::random(rng)),
            _ => unreachable!(),
        }
    }
}

impl Display for NativeTransaction {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        match self {
            NativeTransaction::MintTransfer(_) => write!(formatter, "mint transfer"),
            NativeTransaction::Auction(auction_txn) => write!(formatter, "native: {}", auction_txn),
            NativeTransaction::Reservation(_) => write!(formatter, "reservation"),
        }
    }
}

impl ToBytes for NativeTransaction {
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        match self {
            NativeTransaction::MintTransfer(args) => {
                MINT_TRANSFER_TAG.write_bytes(writer)?;
                args.write_bytes(writer)
            }
            NativeTransaction::Auction(auction_txn) => {
                AUCTION_TAG.write_bytes(writer)?;
                auction_txn.write_bytes(writer)
            }
            NativeTransaction::Reservation(args) => {
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
                NativeTransaction::MintTransfer(args) => args.serialized_length(),
                NativeTransaction::Auction(auction_txn) => auction_txn.serialized_length(),
                NativeTransaction::Reservation(args) => args.serialized_length(),
            }
    }
}

impl FromBytes for NativeTransaction {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (tag, remainder) = u8::from_bytes(bytes)?;
        match tag {
            MINT_TRANSFER_TAG => {
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((NativeTransaction::MintTransfer(args), remainder))
            }
            AUCTION_TAG => {
                let (auction_txn, remainder) = AuctionTransaction::from_bytes(remainder)?;
                Ok((NativeTransaction::Auction(auction_txn), remainder))
            }
            RESERVATION_TAG => {
                let (args, remainder) = RuntimeArgs::from_bytes(remainder)?;
                Ok((NativeTransaction::Reservation(args), remainder))
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
            bytesrepr::test_serialization_roundtrip(&NativeTransaction::random(rng));
        }
    }
}
