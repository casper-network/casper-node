use casper_node::{
    rpcs::{
        chain::{GetBlock},
        RpcWithOptionalParams,
    },
};

use crate::{rpc::RpcClient};

impl RpcClient for GetBlock {
    const RPC_METHOD: &'static str = Self::METHOD;
}
