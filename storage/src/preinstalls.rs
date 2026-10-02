//! EVM utility contracts installed at their canonical addresses at genesis and protocol upgrades.

use alloy_primitives::{hex, keccak256};
use casper_types::evm;

/// A contract whose runtime bytecode is installed directly into EVM state.
#[derive(Clone, Copy, Debug)]
pub struct EvmPreinstall {
    /// Human-readable contract name.
    pub name: &'static str,
    /// Canonical EVM address.
    pub address: evm::Address,
    /// Pinned runtime bytecode, including compiler metadata.
    pub code: &'static [u8],
}

impl EvmPreinstall {
    /// Returns the Keccak-256 hash of the runtime bytecode.
    pub fn code_hash(&self) -> evm::Hash {
        evm::Hash::new(keccak256(self.code).0)
    }
}

/// Canonical Multicall3 preinstall.
///
/// The binary contains the runtime returned by the official signed deployment,
/// rather than its creation bytecode. See `preinstalls/README.md` for provenance.
pub const MULTICALL3: EvmPreinstall = EvmPreinstall {
    name: "Multicall3",
    address: evm::Address::new(hex!("0xcA11bde05977b3631167028862bE2a173976CA11")),
    code: include_bytes!("preinstalls/multicall3.bin"),
};

/// Arachnid's canonical deterministic deployment proxy used by Foundry.
///
/// Calldata is a 32-byte salt followed by creation bytecode. The runtime uses
/// CREATE2 and returns the deployed address as 20 bytes; it needs no initial storage.
pub const CREATE2_DEPLOYER: EvmPreinstall = EvmPreinstall {
    name: "Arachnid CREATE2 deployer",
    address: evm::Address::new(hex!("0x4e59b44847b379578588920cA78FbF26c0B4956C")),
    code: include_bytes!("preinstalls/create2-deployer.bin"),
};

/// Preinstalls upserted after EVM predeploys at EVM-enabled genesis and protocol upgrade commit.
pub const PREINSTALLS: &[EvmPreinstall] = &[
    // Aggregate reads and expose block/chain information at the standard Multicall3 address.
    MULTICALL3,
    // Support deterministic deployments through Foundry's default CREATE2 factory.
    CREATE2_DEPLOYER,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multicall3_matches_canonical_deployment() {
        assert_eq!(
            MULTICALL3.address.to_hex_string(),
            "ca11bde05977b3631167028862be2a173976ca11"
        );
        assert_eq!(MULTICALL3.code.len(), 3_808);
        assert_eq!(
            MULTICALL3.code_hash().to_hex_string(),
            "d5c15df687b16f2ff992fc8d767b4216323184a2bbc6ee2f9c398c318e770891"
        );
    }

    #[test]
    fn create2_deployer_matches_canonical_deployment() {
        assert_eq!(
            CREATE2_DEPLOYER.address.to_hex_string(),
            "4e59b44847b379578588920ca78fbf26c0b4956c"
        );
        assert_eq!(CREATE2_DEPLOYER.code.len(), 69);
        assert_eq!(
            CREATE2_DEPLOYER.code_hash().to_hex_string(),
            "2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989"
        );
    }
}
