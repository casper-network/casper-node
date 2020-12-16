#!/usr/bin/env bash

source $NCTL/sh/utils.sh

unset LOG_LEVEL
unset NET_ID
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        loglevel) LOG_LEVEL=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

LOG_LEVEL=${LOG_LEVEL:-$RUST_LOG}
LOG_LEVEL=${LOG_LEVEL:-debug}
NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-1}

source $(get_path_to_net_vars $NET_ID)

PATH_NET=$(get_path_to_net $NET_ID)
PATH_NET_CHAINSPEC=$PATH_NET/chainspec/chainspec.toml
PATH_NODE=$(get_path_to_node $NET_ID $NODE_ID)
PATH_NODE_CONFIG=$PATH_NET/nodes/node-$NODE_ID/config/node-config.toml
PATH_NODE_STORAGE=$PATH_NODE/storage
PATH_NODE_SECRET_KEY=$PATH_NODE/keys/secret_key.pem

NODE_API_PORT_REST=$(get_node_port_rest $NET_ID $NODE_ID)
NODE_API_PORT_RPC=$(get_node_port_rpc $NET_ID $NODE_ID)
NODE_API_PORT_SSE=$(get_node_port_sse $NET_ID $NODE_ID)

NETWORK_BIND_ADDRESS=$(get_network_bind_address $NET_ID $NODE_ID $NCTL_NET_BOOTSTRAP_COUNT)
NETWORK_KNOWN_ADDRESSES=$(get_network_known_addresses $NET_ID $NCTL_NET_BOOTSTRAP_COUNT)

export RUST_LOG=$LOG_LEVEL

$PATH_NET/bin/casper-node validator $PATH_NODE_CONFIG \
    --config-ext consensus.secret_key_path=$PATH_NODE_SECRET_KEY  \
    --config-ext event_stream_server.address=0.0.0.0:$NODE_API_PORT_SSE  \
    --config-ext logging.format=json \
    --config-ext network.bind_address=$NETWORK_BIND_ADDRESS  \
    --config-ext network.known_addresses=[$NETWORK_KNOWN_ADDRESSES]  \
    --config-ext node.chainspec_config_path=$PATH_NET_CHAINSPEC  \
    --config-ext rest_server.address=0.0.0.0:$NODE_API_PORT_REST  \
    --config-ext rpc_server.address=0.0.0.0:$NODE_API_PORT_RPC  \
    --config-ext storage.path=$PATH_NODE_STORAGE ;
