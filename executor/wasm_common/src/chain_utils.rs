use blake2::{digest::consts::U32, Blake2b, Digest};

/// Compute a predictable address for a contract.
///
/// The address is computed as the hash of the chain name, initiator account, and the hash of the
/// Wasm code.
pub fn compute_predictable_address<T: AsRef<[u8]>>(
    chain_name: T,
    initiator_address: [u8; 32],
    bytecode_hash: [u8; 32],
    seed: Option<[u8; 32]>,
) -> [u8; 32] {
    let mut hasher = Blake2b::<U32>::new();

    hasher.update(chain_name);
    hasher.update(initiator_address);
    hasher.update(bytecode_hash);

    if let Some(seed) = seed {
        hasher.update(seed);
    }

    hasher.finalize().into()
}

pub fn compute_wasm_bytecode_hash<T: AsRef<[u8]>>(wasm_bytes: T) -> [u8; 32] {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(wasm_bytes);
    let hash = hasher.finalize();
    hash.into()
}

#[must_use]
pub fn compute_next_contract_hash_version(
    smart_contract_addr: [u8; 32],
    next_version: u32,
) -> [u8; 32] {
    let mut hasher = Blake2b::<U32>::new();

    hasher.update(smart_contract_addr);
    hasher.update(next_version.to_le_bytes());

    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    const SEED: [u8; 32] = [1u8; 32];

    #[test]
    fn test_compute_predictable_address() {
        let initiator = [1u8; 32];
        let bytecode_hash = [2u8; 32];

        let predictable_address_1 =
            super::compute_predictable_address("testnet", initiator, bytecode_hash, Some(SEED));
        let predictable_address_2 =
            super::compute_predictable_address("mainnet", initiator, bytecode_hash, Some(SEED));
        assert_ne!(predictable_address_1, predictable_address_2);
    }

    #[test]
    fn test_compute_nth_version_hash() {
        let smart_contract_addr = [1u8; 32];
        let mut next_version = 1;

        let hash_1 = super::compute_next_contract_hash_version(smart_contract_addr, next_version);
        next_version += 1;

        let hash_2 = super::compute_next_contract_hash_version(smart_contract_addr, next_version);
        assert_ne!(hash_1, hash_2);
    }
}
