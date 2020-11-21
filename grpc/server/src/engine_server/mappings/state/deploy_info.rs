use std::convert::TryFrom;

use crate::engine_server::{mappings, mappings::ParsingError, state};

use casper_types::{account::AccountHash, DeployHash, DeployInfo, TransferAddr, URef, U512};

impl From<DeployInfo> for state::DeployInfo {
    fn from(deploy_info: DeployInfo) -> Self {
        let mut ret = state::DeployInfo::new();
        {
            let mut pb_deploy_hash = state::DeployHash::new();
            pb_deploy_hash.deploy_hash = deploy_info.deploy_hash.value().to_vec();
            ret.set_deploy(pb_deploy_hash)
        }
        {
            let pb_vec_transfer_addr = deploy_info
                .transfers
                .into_iter()
                .map(|transfer_addr| {
                    let mut pb_transfer_addr = state::TransferAddr::new();
                    pb_transfer_addr.transfer_addr = transfer_addr.to_vec();
                    pb_transfer_addr
                })
                .collect::<Vec<state::TransferAddr>>()
                .into();
            ret.set_transfers(pb_vec_transfer_addr)
        }
        {
            let mut pb_account_hash = state::AccountHash::new();
            pb_account_hash.account_hash = deploy_info.from.value().to_vec();
            ret.set_from(pb_account_hash)
        }
        ret.set_source(deploy_info.source.into());
        ret.set_gas(deploy_info.gas.into());
        ret
    }
}

impl TryFrom<state::DeployInfo> for DeployInfo {
    type Error = ParsingError;

    fn try_from(pb_deploy_info: state::DeployInfo) -> Result<Self, Self::Error> {
        let deploy = {
            let deploy_hash = pb_deploy_info.get_deploy();
            DeployHash::new(mappings::vec_to_array(
                deploy_hash.deploy_hash.to_owned(),
                "Protobuf DeployInfo.deploy",
            )?)
        };
        let transfers = pb_deploy_info
            .get_transfers()
            .iter()
            .map(|pb_transfer_addr| {
                mappings::vec_to_array(
                    pb_transfer_addr.transfer_addr.to_owned(),
                    "Protobuf DeployInfo.transfers",
                )
            })
            .collect::<Result<Vec<TransferAddr>, Self::Error>>()?;
        let from = {
            let account_hash = pb_deploy_info.get_from();
            mappings::vec_to_array(
                account_hash.account_hash.to_owned(),
                "Protobuf DeployInfo.from",
            )
            .map(AccountHash::new)?
        };
        let source = URef::try_from(pb_deploy_info.get_source().to_owned())?;
        let gas = U512::try_from(pb_deploy_info.get_gas().to_owned())?;

        Ok(DeployInfo {
            deploy_hash: deploy,
            transfers,
            from,
            source,
            gas,
        })
    }
}
