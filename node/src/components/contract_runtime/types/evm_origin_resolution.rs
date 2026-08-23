use casper_storage::data_access_layer::BalanceIdentifier;
use casper_types::evm::IdentityInstruction as EvmIdentityInstruction;

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
    #[allow(unused)] // TODO remove this when ready
    identity_plan: EvmIdentityInstruction,
}

impl EvmOriginResolution {
    pub(crate) fn new(
        balance_identifier: BalanceIdentifier,
        identity_plan: EvmIdentityInstruction,
    ) -> Self {
        Self {
            balance_identifier,
            identity_plan,
        }
    }

    pub(crate) fn balance_identifier(&self) -> BalanceIdentifier {
        self.balance_identifier.clone()
    }

    #[allow(unused)] // TODO remove this when ready
    pub(crate) fn identity_plan(&self) -> EvmIdentityInstruction {
        self.identity_plan
    }
}
