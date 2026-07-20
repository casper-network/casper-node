use casper_storage::data_access_layer::BalanceIdentifier;
use casper_types::{account::AccountHash, evm::Address as EvmAddress};

/// Deferred write needed to make an EVM sender's identity explicit in global state.
///
/// The runtime makes this decision because it has both pieces of context the
/// executor should not need: the recovered transaction signer and the Casper
/// account view at the current state root.
#[derive(Clone, Copy, Debug)]
pub(crate) enum EvmIdentityPlan {
    /// No identity write is needed. Either the identity already exists, or the
    /// address must remain EVM-native.
    None,
    /// The EVM address has no identity pointer yet, but the recovered signer
    /// already has a Casper account. Link the address to that account hash.
    LinkExisting {
        address: EvmAddress,
        account_hash: AccountHash,
    },
    /// Neither an identity pointer nor a Casper account exists for the
    /// recovered signer. Create the Casper account and then link the EVM
    /// address to it.
    CreateAccount {
        address: EvmAddress,
        account_hash: AccountHash,
        main_purse: casper_types::URef,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct EvmOriginResolution {
    // Concrete payer selected before payment checks. This is deliberately a
    // data-access balance identifier, not an EVM-specific balance mode, so
    // the rest of block execution can use the normal hold/refund/fee
    // machinery.
    balance_identifier: BalanceIdentifier,
    // State mutation to perform later, inside the same tracking copy as EVM
    // execution. Origin resolution itself is read-only so a rejected
    // transaction does not create accounts or links as a side effect.
    identity_plan: EvmIdentityPlan,
}

impl EvmOriginResolution {
    pub(crate) fn new(
        balance_identifier: BalanceIdentifier,
        identity_plan: EvmIdentityPlan,
    ) -> Self {
        Self {
            balance_identifier,
            identity_plan,
        }
    }

    pub(crate) fn balance_identifier(&self) -> BalanceIdentifier {
        self.balance_identifier.clone()
    }

    pub(crate) fn identity_plan(&self) -> EvmIdentityPlan {
        self.identity_plan
    }
}
