use crate::bytesrepr::{Error, FromBytes, ToBytes};
use alloc::vec::Vec;
use core::fmt::Debug;

const CONTRACT_WASM_MAX_DISPLAY_LEN: usize = 16;

/// A container for contract's WASM bytes.
#[derive(PartialEq, Eq, Clone)]
pub struct ContractWasm {
    bytes: Vec<u8>,
}

impl Debug for ContractWasm {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.bytes.len() > CONTRACT_WASM_MAX_DISPLAY_LEN {
            write!(
                f,
                "ContractWasm(0x{}...)",
                base16::encode_lower(&self.bytes[..CONTRACT_WASM_MAX_DISPLAY_LEN])
            )
        } else {
            write!(f, "ContractWasm(0x{})", base16::encode_lower(&self.bytes))
        }
    }
}

impl ContractWasm {
    /// Creates new WASM object from bytes.
    pub fn new(bytes: Vec<u8>) -> Self {
        ContractWasm { bytes }
    }

    /// Consumes instance of [`ContractWasm`] and returns its bytes.
    pub fn take_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// Returns a slice of contained WASM bytes.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl ToBytes for ContractWasm {
    fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.bytes.to_bytes()
    }

    fn serialized_length(&self) -> usize {
        self.bytes.serialized_length()
    }
}

impl FromBytes for ContractWasm {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), Error> {
        let (bytes, rem1) = Vec::<u8>::from_bytes(bytes)?;
        Ok((ContractWasm { bytes }, rem1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_debug_repr_of_short_wasm() {
        const SIZE: usize = 8;
        let wasm_bytes = vec![0; SIZE];
        let contract_wasm = ContractWasm::new(wasm_bytes);
        // String output is less than the bytes itself
        assert_eq!(
            format!("{:?}", contract_wasm),
            "ContractWasm(0x0000000000000000)"
        );
    }

    #[test]
    fn test_debug_repr_of_long_wasm() {
        const SIZE: usize = 65;
        let wasm_bytes = vec![0; SIZE];
        let contract_wasm = ContractWasm::new(wasm_bytes);
        // String output is less than the bytes itself
        assert_eq!(
            format!("{:?}", contract_wasm),
            "ContractWasm(0x00000000000000000000000000000000...)"
        );
    }
}
