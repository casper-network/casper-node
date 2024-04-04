use std::collections::BTreeMap;
#[cfg(test)]
use std::{collections::VecDeque, iter::FromIterator};

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    global_state::TrieMerkleProof,
    system::mint::BalanceHoldAddrTag,
    BlockTime, Key, StoredValue, U512,
};
#[cfg(test)]
use casper_types::{global_state::TrieMerkleProofStep, CLValue};
#[cfg(test)]
use rand::Rng;

/// Response to a balance query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalanceResponse {
    /// The purses total balance, not considering holds.
    pub total_balance: U512,
    /// The available balance (total balance - sum of all active holds).
    pub available_balance: U512,
    /// A proof that the given value is present in the Merkle trie.
    pub total_balance_proof: Box<TrieMerkleProof<Key, StoredValue>>,
    /// Any time-relevant active holds on the balance.
    pub balance_holds: BTreeMap<BlockTime, BalanceHoldsWithProof>,
}

impl BalanceResponse {
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        BalanceResponse {
            total_balance: rng.gen(),
            available_balance: rng.gen(),
            total_balance_proof: Box::new(TrieMerkleProof::new(
                Key::URef(rng.gen()),
                StoredValue::CLValue(CLValue::from_t(rng.gen::<i32>()).unwrap()),
                VecDeque::from_iter([TrieMerkleProofStep::random(rng)]),
            )),
            balance_holds: BTreeMap::new(),
        }
    }
}

impl ToBytes for BalanceResponse {
    fn to_bytes(&self) -> Result<Vec<u8>, casper_types::bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), casper_types::bytesrepr::Error> {
        self.total_balance.write_bytes(writer)?;
        self.available_balance.write_bytes(writer)?;
        self.total_balance_proof.write_bytes(writer)?;
        self.balance_holds.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.total_balance.serialized_length()
            + self.available_balance.serialized_length()
            + self.total_balance_proof.serialized_length()
            + self.balance_holds.serialized_length()
    }
}

impl FromBytes for BalanceResponse {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), casper_types::bytesrepr::Error> {
        let (total_balance, remainder) = U512::from_bytes(bytes)?;
        let (available_balance, remainder) = U512::from_bytes(remainder)?;
        let (total_balance_proof, remainder) =
            TrieMerkleProof::<Key, StoredValue>::from_bytes(remainder)?;
        let (balance_holds, remainder) =
            BTreeMap::<BlockTime, BalanceHoldsWithProof>::from_bytes(remainder)?;
        Ok((
            BalanceResponse {
                total_balance,
                available_balance,
                total_balance_proof: Box::new(total_balance_proof),
                balance_holds,
            },
            remainder,
        ))
    }
}

/// Balance holds with Merkle proofs.
pub type BalanceHoldsWithProof =
    BTreeMap<BalanceHoldAddrTag, (U512, TrieMerkleProof<Key, StoredValue>)>;

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = BalanceResponse::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
