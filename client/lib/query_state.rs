use casper_node::rpcs::{state::GetItem, RpcWithParams};

use crate::rpc::RpcClient;

impl RpcClient for GetItem {
    const RPC_METHOD: &'static str = <Self as RpcWithParams>::METHOD;
}
