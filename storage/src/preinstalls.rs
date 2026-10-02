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

/// Preinstalls upserted after EVM predeploys at EVM-enabled genesis and protocol upgrade commit.
pub const PREINSTALLS: &[EvmPreinstall] = &[MULTICALL3];

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
}
