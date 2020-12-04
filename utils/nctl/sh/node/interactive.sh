#!/usr/bin/env bash
#
# Interactively spins up a node within a network.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
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

# Set defaults.
LOG_LEVEL=${LOG_LEVEL:-$RUST_LOG}
LOG_LEVEL=${LOG_LEVEL:-debug}
NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-1}

# Set rust log level.
export RUST_LOG=$LOG_LEVEL

#######################################
# Imports
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Import net vars.
source $(get_path_to_net_vars $NET_ID)

#######################################
# Main
#######################################

# Set paths.
PATH_NET=$(get_path_to_net $NET_ID)
PATH_NET_CHAINSPEC=$PATH_NET/chainspec/chainspec.toml
PATH_NODE=$(get_path_to_node $NET_ID $NODE_ID)
PATH_NODE_CONFIG=$PATH_NET/nodes/node-$NODE_ID/config/node-config.toml
PATH_NODE_STORAGE=$PATH_NODE/storage
PATH_NODE_SECRET_KEY=$PATH_NODE/keys/secret_key.pem

# Set ports.
NODE_API_PORT_REST=$(get_node_port_rest $NET_ID $NODE_ID)
NODE_API_PORT_RPC=$(get_node_port_rpc $NET_ID $NODE_ID)
NODE_API_PORT_SSE=$(get_node_port_sse $NET_ID $NODE_ID)

# Set validator network addresses.
NETWORK_BIND_ADDRESS=$(get_network_bind_address $NET_ID $NODE_ID $NCTL_NET_BOOTSTRAP_COUNT)
NETWORK_KNOWN_ADDRESSES=$(get_network_known_addresses $NET_ID $NODE_ID $NCTL_NET_BOOTSTRAP_COUNT)

# Start node in validator mode.
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
