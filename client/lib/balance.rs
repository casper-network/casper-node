use casper_node::rpcs::{state::GetBalance, RpcWithParams};

use crate::rpc::RpcClient;

impl RpcClient for GetBalance {
    const RPC_METHOD: &'static str = Self::METHOD;
}
