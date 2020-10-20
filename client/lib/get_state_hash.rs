use casper_node::rpcs::{chain::GetStateRootHash, RpcWithOptionalParams};

use crate::rpc::RpcClient;

impl RpcClient for GetStateRootHash {
    const RPC_METHOD: &'static str = Self::METHOD;
}
