use casper_types::{
    bytesrepr,
    bytesrepr::{FromBytes, ToBytes},
    contract_messages::Messages,
    execution::Effects,
    Gas, InvalidTransaction, TransferAddr,
};

#[derive(Debug)]
pub struct SpeculativeExecutionResult {
    /// List of transfers that happened during execution.
    transfers: Vec<TransferAddr>,
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
        transfers: Vec<TransferAddr>,
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
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.transfers.write_bytes(writer)?;
        self.limit.write_bytes(writer)?;
        self.consumed.write_bytes(writer)?;
        self.effects.write_bytes(writer)?;
        self.messages.write_bytes(writer)?;
        self.error.write_bytes(writer)
    }
}

impl FromBytes for SpeculativeExecutionResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (transfers, bytes) = Vec::<TransferAddr>::from_bytes(bytes)?;
        let (limit, bytes) = Gas::from_bytes(bytes)?;
        let (consumed, bytes) = Gas::from_bytes(bytes)?;
        let (effects, bytes) = Effects::from_bytes(bytes)?;
        let (messages, bytes) = Messages::from_bytes(bytes)?;
        let (error, bytes) = Option::<String>::from_bytes(bytes)?;
        Ok((
            SpeculativeExecutionResult {
                transfers,
                limit,
                consumed,
                effects,
                messages,
                error,
            },
            bytes,
        ))
    }
}
//
// #[cfg(test)]
// mod tests {
//     // use super::*;
//     // use casper_types::testing::TestRng;
//
//     #[test]
//     fn bytesrepr_roundtrip() {
//         todo!();
//         let rng = &mut TestRng::new();
//
//         let val = SpeculativeExecutionResult::random(rng);
//         bytesrepr::test_serialization_roundtrip(&val);
//     }
// }
