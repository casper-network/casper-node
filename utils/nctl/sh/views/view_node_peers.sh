#!/usr/bin/env bash
#
# Renders node peers to stdout.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

#######################################
# Displays to stdout current node peers.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_node_peers() {
    node_address=$(get_node_address_rpc $1 $2)
    log "network #$1 :: node #$2 :: $node_address :: peers:"
    curl -s --header 'Content-Type: application/json' \
        --request POST $(get_node_address_rpc_for_curl $1 $2) \
        --data-raw '{
            "id": 1,
            "jsonrpc": "2.0",
            "method": "info_get_peers"
        }' | jq '.result.peers'
}

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset net
unset node

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        *)
    esac
done

# Set defaults.
net=${net:-1}
node=${node:-"all"}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils/misc.sh

# Import vars.
source $(get_path_to_net_vars $net)

# Render peer set.
if [ $node = "all" ]; then
    for idx in $(seq 1 $NCTL_NET_NODE_COUNT)
    do
        echo "------------------------------------------------------------------------------------------------------------------------------------"
        render_node_peers $net $idx
    done
    echo "------------------------------------------------------------------------------------------------------------------------------------"
else
    render_node_peers $net $node
fi
