use blake2::Blake2b;
use casper_types::{account::AccountHash, HashAddr};
use digest::{consts::U32, Digest};

/// Compute a predictable address for a contract.
///
/// The address is computed as the hash of the chain name, initiator account, and the hash of the
/// Wasm code.
pub(crate) fn compute_predictable_address<T: AsRef<[u8]>>(
    chain_name: T,
    initiator: AccountHash,
    bytecode_hash: HashAddr,
    seed: Option<[u8; 32]>,
) -> HashAddr {
    let contract_hash: HashAddr = {
        let mut hasher = Blake2b::<U32>::new();

        hasher.update(chain_name);
        hasher.update(initiator);
        hasher.update(bytecode_hash);
        if let Some(seed) = seed {
            hasher.update(seed);
        }

        hasher.finalize().into()
    };
    contract_hash
}

pub(crate) fn compute_wasm_bytecode_hash<T: AsRef<[u8]>>(wasm_bytes: T) -> HashAddr {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(wasm_bytes);
    let hash = hasher.finalize();
    hash.into()
}

#[cfg(test)]
mod tests {
    use casper_types::account::AccountHash;

    #[test]
    fn test_compute_predictable_address() {
        const SEED: [u8; 32] = [1u8; 32];
        let initiator = AccountHash::new([1u8; 32]);
        let bytecode_hash = [2u8; 32];

        let predictable_address_1 =
            super::compute_predictable_address("testnet", initiator, bytecode_hash, Some(SEED));
        let predictable_address_2 =
            super::compute_predictable_address("mainnet", initiator, bytecode_hash, Some(SEED));
        assert_ne!(predictable_address_1, predictable_address_2);
    }
}
