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

/// Safe's canonical singleton factory for deterministic contract deployments.
///
/// It has the same salt/creation-code interface as the Arachnid deployer, at
/// the address used by Safe's chain-specific signed deployment transactions.
pub const SAFE_SINGLETON_FACTORY: EvmPreinstall = EvmPreinstall {
    name: "Safe Singleton Factory",
    address: evm::Address::new(hex!("0x914d7Fec6aaC8cd542e72Bca78B30650d45643d7")),
    code: include_bytes!("preinstalls/safe-singleton-factory.bin"),
};

/// ERC-2470's canonical singleton factory with the `deploy(bytes,bytes32)` ABI.
///
/// Deployments use CREATE2 without forwarding value. Unlike the raw factories,
/// a failed deployment returns the zero address instead of reverting.
pub const ERC2470_SINGLETON_FACTORY: EvmPreinstall = EvmPreinstall {
    name: "ERC-2470 Singleton Factory",
    address: evm::Address::new(hex!("0xce0042B868300000d44A59004Da54A005ffdcf9f")),
    code: include_bytes!("preinstalls/erc2470-singleton-factory.bin"),
};

/// Uniswap's canonical Permit2 token approval and signature-transfer contract.
///
/// The mainnet runtime includes the constructor's cached EIP-712 domain. It
/// recomputes that domain when the current chain ID differs from the cached one.
pub const PERMIT2: EvmPreinstall = EvmPreinstall {
    name: "Permit2",
    address: evm::Address::new(hex!("0x000000000022D473030F116dDEE9F6B43aC78BA3")),
    code: include_bytes!("preinstalls/permit2.bin"),
};

/// Canonical ERC-4337 EntryPoint v0.8 for processing UserOperations.
///
/// The deployed runtime contains its SenderCreator address and EIP-712
/// constructor immutables. It uses transient storage supported by Prague.
pub const ENTRYPOINT_V08: EvmPreinstall = EvmPreinstall {
    name: "EntryPoint v0.8",
    address: evm::Address::new(hex!("0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108")),
    code: include_bytes!("preinstalls/entrypoint-v08.bin"),
};

/// Account-creation helper from the canonical [`ENTRYPOINT_V08`] deployment.
///
/// Its runtime authorizes only EntryPoint at
/// `0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108`. Install the matching pair together;
/// substituting an unlinked compiler artifact would lose that authorization.
pub const SENDER_CREATOR_V08: EvmPreinstall = EvmPreinstall {
    name: "SenderCreator v0.8",
    address: evm::Address::new(hex!("0x449ED7C3e6Fee6a97311d4b55475DF59C44AdD33")),
    code: include_bytes!("preinstalls/sendercreator-v08.bin"),
};

/// Preinstalls upserted after EVM predeploys at EVM-enabled genesis and protocol upgrades.
///
/// EntryPoint and SenderCreator v0.8 are a matching pair activated in the same
/// genesis or protocol upgrade commit. No constructors or contract calls execute
/// while iterating this list, so their relative order does not affect the state.
pub const PREINSTALLS: &[EvmPreinstall] = &[
    // Aggregate reads and expose block/chain information at the standard Multicall3 address.
    MULTICALL3,
    // Support deterministic deployments through Foundry's default CREATE2 factory.
    CREATE2_DEPLOYER,
    // Support deterministic deployments at the factory address used by Safe tooling.
    SAFE_SINGLETON_FACTORY,
    // Provide the standard ABI-based, zero-value CREATE2 factory from ERC-2470.
    ERC2470_SINGLETON_FACTORY,
    // Provide Uniswap's shared ERC-20 allowance and signature-transfer infrastructure.
    PERMIT2,
    // Create smart accounts only on behalf of the canonical EntryPoint v0.8.
    SENDER_CREATOR_V08,
    // Process ERC-4337 UserOperations using the matching SenderCreator v0.8 above.
    ENTRYPOINT_V08,
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

    #[test]
    fn safe_singleton_factory_matches_canonical_deployment() {
        assert_eq!(
            SAFE_SINGLETON_FACTORY.address.to_hex_string(),
            "914d7fec6aac8cd542e72bca78b30650d45643d7"
        );
        assert_eq!(SAFE_SINGLETON_FACTORY.code.len(), 69);
        assert_eq!(
            SAFE_SINGLETON_FACTORY.code_hash().to_hex_string(),
            "2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989"
        );
        assert_eq!(SAFE_SINGLETON_FACTORY.code, CREATE2_DEPLOYER.code);
    }

    #[test]
    fn erc2470_singleton_factory_matches_canonical_deployment() {
        assert_eq!(
            ERC2470_SINGLETON_FACTORY.address.to_hex_string(),
            "ce0042b868300000d44a59004da54a005ffdcf9f"
        );
        assert_eq!(ERC2470_SINGLETON_FACTORY.code.len(), 308);
        assert_eq!(
            ERC2470_SINGLETON_FACTORY.code_hash().to_hex_string(),
            "c4d5542b53a8b779595a20a8ddd60e58a6c49d3c3decc2df83ced1c69c8ca807"
        );
    }

    #[test]
    fn permit2_matches_canonical_deployment() {
        assert_eq!(
            PERMIT2.address.to_hex_string(),
            "000000000022d473030f116ddee9f6b43ac78ba3"
        );
        assert_eq!(PERMIT2.code.len(), 9_152);
        assert_eq!(
            PERMIT2.code_hash().to_hex_string(),
            "c67d1657868aa5146eaf24fb879fb1fdec3d2d493b3683a61c9c2f4fb2851131"
        );
    }

    #[test]
    fn sender_creator_v08_matches_canonical_deployment() {
        assert_eq!(
            SENDER_CREATOR_V08.address.to_hex_string(),
            "449ed7c3e6fee6a97311d4b55475df59c44add33"
        );
        assert_eq!(SENDER_CREATOR_V08.code.len(), 1_217);
        assert_eq!(
            SENDER_CREATOR_V08.code_hash().to_hex_string(),
            "c69a1b3a000d570bc86eb096ee63a9014a17951ad616d720882ec61432b00fcf"
        );
    }

    #[test]
    fn entrypoint_v08_matches_canonical_deployment() {
        assert_eq!(
            ENTRYPOINT_V08.address.to_hex_string(),
            "4337084d9e255ff0702461cf8895ce9e3b5ff108"
        );
        assert_eq!(ENTRYPOINT_V08.code.len(), 21_738);
        assert_eq!(
            ENTRYPOINT_V08.code_hash().to_hex_string(),
            "44e632a24c6f2600cbd5b5b8b4c2d372359112c8b5774297f5fd0a9e64f11f86"
        );
    }

    #[test]
    fn preinstall_addresses_are_unique() {
        let mut addresses = std::collections::BTreeSet::new();
        for preinstall in PREINSTALLS {
            assert!(
                addresses.insert(preinstall.address),
                "duplicate preinstall address: {}",
                preinstall.address,
            );
        }
    }
}
