#!/usr/bin/env bash
#
# Renders a network faucet account key.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.

# Import utils.
source $NCTL/sh/utils/misc.sh

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
node=${node:-1}

#######################################
# Main
#######################################

curl -s --header 'Content-Type: application/json' \
    --request POST $(get_node_address_rpc $net $node) \
    --data-raw '{
        "id": 1,
        "jsonrpc": "2.0",
        "method": "chain_get_block"
    }' \
    | jq '.result.block.header.state_root_hash' \
    | sed -e 's/^"//' -e 's/"$//'
