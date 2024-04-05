use once_cell::sync::Lazy;
#[cfg(any(feature = "testing", test))]
use rand::Rng;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[cfg(any(feature = "testing", test))]
use rand::distributions::{Alphanumeric, DistString};

#[cfg(any(feature = "testing", test))]
use casper_types::testing::TestRng;
use casper_types::{
    bytesrepr::{self, FromBytes, ToBytes},
    contract_messages::Messages,
    execution::Effects,
    BlockHash, Digest, Gas, InvalidTransaction, Transfer,
};

static SPECULATIVE_EXECUTION_RESULT: Lazy<SpeculativeExecutionResult> = Lazy::new(|| {
    SpeculativeExecutionResult::new(
        BlockHash::new(Digest::from([0; Digest::LENGTH])),
        vec![],
        Gas::zero(),
        Gas::zero(),
        Effects::new(),
        Messages::new(),
        None,
    )
});

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize, JsonSchema)]
pub struct SpeculativeExecutionResult {
    /// Block hash against which the execution was performed.
    block_hash: BlockHash,
    /// List of transfers that happened during execution.
    transfers: Vec<Transfer>,
    /// Gas limit.
    limit: Gas,
    /// Gas consumed.
    consumed: Gas,
    /// Execution effects.
    effects: Effects,
    /// Messages emitted during execution.
    messages: Messages,
    /// Did the wasm execute successfully?
    error: Option<String>,
}

impl SpeculativeExecutionResult {
    pub fn new(
        block_hash: BlockHash,
        transfers: Vec<Transfer>,
        limit: Gas,
        consumed: Gas,
        effects: Effects,
        messages: Messages,
        error: Option<String>,
    ) -> Self {
        SpeculativeExecutionResult {
            transfers,
            limit,
            consumed,
            effects,
            messages,
            error,
            block_hash,
        }
    }

    // This method is not intended to be used by third party crates.
    #[doc(hidden)]
    pub fn example() -> &'static Self {
        &SPECULATIVE_EXECUTION_RESULT
    }

    #[cfg(any(feature = "testing", test))]
    pub fn random(rng: &mut TestRng) -> Self {
        use casper_types::contract_messages::Message;

        let random_messages = |rng: &mut TestRng| -> Messages {
            let count = rng.gen_range(16..128);
            std::iter::repeat_with(|| Message::random(rng))
                .take(count)
                .collect()
        };

        SpeculativeExecutionResult {
            block_hash: BlockHash::new(rng.gen()),
            transfers: vec![Transfer::random(rng)],
            limit: Gas::random(rng),
            consumed: Gas::random(rng),
            effects: Effects::random(rng),
            messages: random_messages(rng),
            error: if rng.gen() {
                None
            } else {
                let count = rng.gen_range(16..128);
                Some(Alphanumeric.sample_string(rng, count))
            },
        }
    }
}

impl From<InvalidTransaction> for SpeculativeExecutionResult {
    fn from(invalid_transaction: InvalidTransaction) -> Self {
        SpeculativeExecutionResult {
            transfers: Default::default(),
            limit: Default::default(),
            consumed: Default::default(),
            effects: Default::default(),
            messages: Default::default(),
            error: Some(format!("{}", invalid_transaction)),
            block_hash: Default::default(),
        }
    }
}

impl ToBytes for SpeculativeExecutionResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        ToBytes::serialized_length(&self.transfers)
            + ToBytes::serialized_length(&self.limit)
            + ToBytes::serialized_length(&self.consumed)
            + ToBytes::serialized_length(&self.effects)
            + ToBytes::serialized_length(&self.messages)
            + ToBytes::serialized_length(&self.error)
            + ToBytes::serialized_length(&self.block_hash)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.transfers.write_bytes(writer)?;
        self.limit.write_bytes(writer)?;
        self.consumed.write_bytes(writer)?;
        self.effects.write_bytes(writer)?;
        self.messages.write_bytes(writer)?;
        self.error.write_bytes(writer)?;
        self.block_hash.write_bytes(writer)
    }
}

impl FromBytes for SpeculativeExecutionResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (transfers, bytes) = Vec::<Transfer>::from_bytes(bytes)?;
        let (limit, bytes) = Gas::from_bytes(bytes)?;
        let (consumed, bytes) = Gas::from_bytes(bytes)?;
        let (effects, bytes) = Effects::from_bytes(bytes)?;
        let (messages, bytes) = Messages::from_bytes(bytes)?;
        let (error, bytes) = Option::<String>::from_bytes(bytes)?;
        let (block_hash, bytes) = BlockHash::from_bytes(bytes)?;
        Ok((
            SpeculativeExecutionResult {
                transfers,
                limit,
                consumed,
                effects,
                messages,
                error,
                block_hash,
            },
            bytes,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use casper_types::testing::TestRng;

    #[test]
    fn bytesrepr_roundtrip() {
        let rng = &mut TestRng::new();

        let val = SpeculativeExecutionResult::random(rng);
        bytesrepr::test_serialization_roundtrip(&val);
    }
}
