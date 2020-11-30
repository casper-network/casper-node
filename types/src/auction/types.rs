use alloc::{collections::BTreeMap, vec::Vec};

use crate::{
    auction::{Bid, SeigniorageRecipient, UnbondingPurse},
    PublicKey, U512,
};

/// Representation of delegation rate of tokens. Fraction of 1 in trillionths (12 decimal places).
pub type DelegationRate = u64;

/// Validators mapped to their bids.
pub type Bids = BTreeMap<PublicKey, Bid>;

/// Weights of validators. "Weight" in this context means a sum of their stakes.
pub type ValidatorWeights = BTreeMap<PublicKey, U512>;

/// Era index type.
pub type EraId = u64;

/// List of era validators
pub type EraValidators = BTreeMap<EraId, ValidatorWeights>;

/// Collection of seigniorage recipients.
pub type SeigniorageRecipients = BTreeMap<PublicKey, SeigniorageRecipient>;

/// Snapshot of `SeigniorageRecipients` for a given era.
pub type SeigniorageRecipientsSnapshot = BTreeMap<EraId, SeigniorageRecipients>;

/// Validators and delegators mapped to their unbonding purses.
pub type UnbondingPurses = BTreeMap<PublicKey, Vec<UnbondingPurse>>;
