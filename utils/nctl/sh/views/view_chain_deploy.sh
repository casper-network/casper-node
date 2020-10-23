#!/usr/bin/env bash
#
# Renders on-chain deploy data to stdout.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Deploy hash.

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset deploy
unset net
unset node

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        deploy) deploy_hash=${VALUE} ;;
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
log "network #$net :: node #$node :: $node_address :: deploy:"
curl -s --header 'Content-Type: application/json' \
    --request POST $(get_node_address_rpc $net $node) \
    --data-raw '{
        "id": 1,
        "jsonrpc": "2.0",
        "method": "info_get_deploy",
        "params": {
            "deploy_hash":"'$deploy_hash'"
        }
    }' \
    | jq '.result'
