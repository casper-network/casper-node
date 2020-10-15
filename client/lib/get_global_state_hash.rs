use casper_node::{
    rpcs::{
        chain::{GetGlobalStateHash},
        RpcWithOptionalParams,
    },
};

use crate::{rpc::RpcClient};

impl RpcClient for GetGlobalStateHash {
    const RPC_METHOD: &'static str = Self::METHOD;
}