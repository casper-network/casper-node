use crate::account::AccountHash;

/// Deferred write needed to make an EVM sender's identity explicit in global state.
///
/// The runtime makes this decision because it has both pieces of context the
/// executor should not need: the recovered transaction signer and the Casper
/// account view at the current state root.
#[derive(Clone, Copy, Debug)]
pub enum IdentityInstruction {
    /// No identity write is needed. Either the identity already exists, or the
    /// address must remain EVM-native.
    None,
    /// The EVM address has no identity pointer yet, but the recovered signer
    /// already has a Casper account. Link the address to that account hash.
    LinkExisting {
        address: super::Address,
        account_hash: AccountHash,
    },
    /// Neither an identity pointer nor a Casper account exists for the
    /// recovered signer. Create the Casper account and then link the EVM
    /// address to it.
    CreateAccount {
        address: super::Address,
        account_hash: AccountHash,
        main_purse: crate::URef,
    },
}
