use std::convert::TryFrom;

use casper_types::{account::AccountHash, Transfer, URef, U512};

use crate::engine_server::{mappings, mappings::ParsingError, state};

impl From<u64> for state::TransferId {
    fn from(value: u64) -> Self {
        let mut ret = Self::new();
        ret.set_value(value);
        ret
    }
}

impl From<Transfer> for state::Transfer {
    fn from(transfer: Transfer) -> Self {
        let mut ret = Self::new();
        {
            let mut pb_deploy_hash = state::DeployHash::new();
            pb_deploy_hash.deploy_hash = transfer.deploy_hash.to_vec();
            ret.set_deploy(pb_deploy_hash);
        }
        {
            let mut pb_account_hash = state::AccountHash::new();
            pb_account_hash.account_hash = transfer.from.value().to_vec();
            ret.set_from(pb_account_hash);
        }
        ret.set_source(transfer.source.into());
        ret.set_target(transfer.target.into());
        ret.set_amount(transfer.amount.into());
        ret.set_gas(transfer.gas.into());
        if let Some(value) = transfer.id {
            ret.set_id(value.into());
        }
        ret
    }
}

impl TryFrom<state::Transfer> for Transfer {
    type Error = ParsingError;

    fn try_from(pb_transfer: state::Transfer) -> Result<Self, Self::Error> {
        let deploy_hash = {
            let pb_deploy_hash = pb_transfer.get_deploy();
            mappings::vec_to_array(
                pb_deploy_hash.deploy_hash.to_owned(),
                "Protobuf Transfer.deploy",
            )?
        };
        let from = {
            let pb_account_hash = pb_transfer.get_from();
            mappings::vec_to_array(
                pb_account_hash.account_hash.to_owned(),
                "Protobuf Transfer.from",
            )
            .map(AccountHash::new)?
        };
        let source = URef::try_from(pb_transfer.get_source().to_owned())?;
        let target = URef::try_from(pb_transfer.get_target().to_owned())?;
        let amount = U512::try_from(pb_transfer.get_amount().to_owned())?;
        let gas = U512::try_from(pb_transfer.get_gas().to_owned())?;
        let id: Option<u64> = if pb_transfer.has_id() {
            Some(pb_transfer.get_id().get_value())
        } else {
            None
        };
        Ok(Transfer {
            deploy_hash,
            from,
            source,
            target,
            amount,
            gas,
            id,
        })
    }
}
