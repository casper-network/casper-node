use blake2::Blake2b;
use digest::{consts::U32, Digest};

use crate::storage::Address;

/// Compute a predictable address for a contract.
///
/// The address is computed as the hash of the chain name, initiator account, and the hash of the
/// Wasm code. This is used to ensure that the same contract is deployed at the same address on
/// different chains.
pub(crate) fn compute_predictable_address<T: AsRef<[u8]>>(
    chain_name: T,
    initiator: Address,
    bytecode_hash: Address,
) -> Address {
    let contract_hash: Address = {
        let mut hasher = Blake2b::<U32>::new();

        hasher.update(chain_name);
        hasher.update(&initiator);
        hasher.update(&bytecode_hash);
        // TODO: Seed to distinguish same code

        hasher.finalize().into()
    };
    contract_hash
}

pub(crate) fn compute_wasm_bytecode_hash<T: AsRef<[u8]>>(wasm_bytes: T) -> Address {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(wasm_bytes);
    let hash = hasher.finalize();
    hash.into()
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_compute_predictable_address() {
        let initiator = [1u8; 32];
        let bytecode_hash = [2u8; 32];

        let predictable_address_1 =
            super::compute_predictable_address("testnet", initiator, bytecode_hash);
        let predictable_address_2 =
            super::compute_predictable_address("mainnet", initiator, bytecode_hash);
        assert_ne!(predictable_address_1, predictable_address_2);
    }
}
