#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

function main() 
{
    local NODE_ID=${1}
    local LOG_LEVEL=${2}

    local PATH_CHAINSPEC
    local PATH_NODE
    local PATH_NODE_CONFIG
    local PATH_NODE_STORAGE
    local PATH_NODE_SECRET_KEY
    local NODE_API_PORT_REST
    local NODE_API_PORT_RPC
    local NODE_API_PORT_SSE
    local NETWORK_BIND_ADDRESS
    local NETWORK_KNOWN_ADDRESSES

    PATH_CHAINSPEC=$(get_path_to_net)/chainspec/chainspec.toml
    PATH_NODE=$(get_path_to_node "$NODE_ID")
    PATH_NODE_CONFIG=$(get_path_to_net)/nodes/node-$NODE_ID/config/node-config.toml
    PATH_NODE_STORAGE=$PATH_NODE/storage
    PATH_NODE_SECRET_KEY=$PATH_NODE/keys/secret_key.pem

    NODE_API_PORT_REST=$(get_node_port_rest "$NODE_ID")
    NODE_API_PORT_RPC=$(get_node_port_rpc "$NODE_ID")
    NODE_API_PORT_SSE=$(get_node_port_sse "$NODE_ID")

    NETWORK_BIND_ADDRESS=$(get_network_bind_address "$NODE_ID")
    NETWORK_KNOWN_ADDRESSES=[$(get_network_known_addresses)]

    export RUST_LOG=$LOG_LEVEL

    "$(get_path_to_net)"/bin/casper-node validator "$PATH_NODE_CONFIG" \
        --config-ext consensus.secret_key_path="$PATH_NODE_SECRET_KEY"  \
        --config-ext event_stream_server.address=0.0.0.0:"$NODE_API_PORT_SSE"  \
        --config-ext logging.format=json \
        --config-ext network.bind_address="$NETWORK_BIND_ADDRESS"  \
        --config-ext network.known_addresses="$NETWORK_KNOWN_ADDRESSES"  \
        --config-ext node.chainspec_config_path="$PATH_CHAINSPEC"  \
        --config-ext rest_server.address=0.0.0.0:"$NODE_API_PORT_REST"  \
        --config-ext rpc_server.address=0.0.0.0:"$NODE_API_PORT_RPC"  \
        --config-ext storage.path="$PATH_NODE_STORAGE" ;
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset LOG_LEVEL
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        loglevel) LOG_LEVEL=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

LOG_LEVEL=${LOG_LEVEL:-$RUST_LOG}
LOG_LEVEL=${LOG_LEVEL:-debug}
NODE_ID=${NODE_ID:-1}

main "$NODE_ID" "$LOG_LEVEL"
