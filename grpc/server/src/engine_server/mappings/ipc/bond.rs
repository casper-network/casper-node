use std::convert::{TryFrom, TryInto};

use types::{account::AccountHash, U512};

use crate::engine_server::{ipc::Bond, mappings::MappingError};

impl From<(AccountHash, U512)> for Bond {
    fn from((account_hash, amount): (AccountHash, U512)) -> Self {
        let mut pb_bond = Bond::new();
        pb_bond.validator_account_hash = account_hash.as_bytes().to_vec();
        pb_bond.set_stake(amount.into());
        pb_bond
    }
}

impl TryFrom<Bond> for (AccountHash, U512) {
    type Error = MappingError;

    fn try_from(mut pb_bond: Bond) -> Result<Self, Self::Error> {
        // TODO: our TryFromSliceForAccountHashError should convey length info
        let account_hash =
            AccountHash::try_from(pb_bond.get_validator_account_hash()).map_err(|_| {
                MappingError::invalid_account_hash_length(pb_bond.validator_account_hash.len())
            })?;

        let stake = pb_bond.take_stake().try_into()?;

        Ok((account_hash, stake))
    }
}

#[cfg(test)]
mod tests {
    use proptest::proptest;

    use types::gens;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip(account_hash in gens::account_hash_arb(), u512 in gens::u512_arb()) {
            test_utils::protobuf_round_trip::<(AccountHash, U512), Bond>((account_hash, u512));
        }
    }
}
