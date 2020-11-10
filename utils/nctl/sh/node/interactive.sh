#!/usr/bin/env bash
#
# Interactively spins up a node within a network.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset loglevel
unset net
unset node

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        loglevel) loglevel=${VALUE} ;;
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        *)
    esac
done

# Set defaults.
loglevel=${loglevel:-$RUST_LOG}
loglevel=${loglevel:-debug}
net=${net:-1}
node=${node:-1}

#######################################
# Main
#######################################

# Set rust log level.
export RUST_LOG=$loglevel

# Import net vars.
source $NCTL/assets/net-$net/vars

# Set paths.
path_net=$NCTL/assets/net-$net
path_net_chainspec=$path_net/chainspec/chainspec.toml
path_node=$path_net/nodes/node-$node
path_node_config=$path_net/nodes/node-$node/config/node-config.toml
path_node_storage=$path_node/storage
path_node_secret_key=$path_node/keys/secret_key.pem

# Set ports.
node_api_port_event=$(get_node_port $NCTL_BASE_PORT_EVENT $net $node)
node_api_port_rest=$(get_node_port $NCTL_BASE_PORT_REST $net $node)
node_api_port_rpc=$(get_node_port $NCTL_BASE_PORT_RPC $net $node)

# Set validator network addresses.
network_bind_address=$(get_network_bind_address $net $node $NCTL_NET_BOOTSTRAP_COUNT)
network_known_addresses=$(get_network_known_addresses $net $node $NCTL_NET_BOOTSTRAP_COUNT)

# Start node in validator mode.
$NCTL/assets/net-$net/bin/casper-node validator $path_node_config \
    --config-ext=consensus.secret_key_path=$path_node_secret_key  \
    --config-ext=event_stream_server.address=0.0.0.0:$node_api_port_event  \
    --config-ext network.bind_address=$network_bind_address  \
    --config-ext network.known_addresses=[$network_known_addresses]  \
    --config-ext=node.chainspec_config_path=$path_net_chainspec  \
    --config-ext=rest_server.address=0.0.0.0:$node_api_port_rest  \
    --config-ext=rpc_server.address=0.0.0.0:$node_api_port_rpc  \
    --config-ext=storage.path=$path_node_storage ;
