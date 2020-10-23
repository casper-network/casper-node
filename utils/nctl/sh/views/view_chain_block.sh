#!/usr/bin/env bash
#
# Renders on-chain block data to stdout.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Block hash (optional).

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset block_hash
unset net
unset node

# Destructure named args.
for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        block) block_hash=${VALUE} ;;
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

node_address=$(get_node_address $net $node)
log "network #$net :: node #$node :: $node_address :: block:"
curl -s --header 'Content-Type: application/json' \
    --request POST $(get_node_address_rpc $net $node) \
    --data-raw '{
        "id": 1,
        "jsonrpc": "2.0",
        "method": "chain_get_block",
        "params": {
            "block_hash":"'$block_hash'"
        }
    }' \
    | jq '.result.block'
