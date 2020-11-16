#!/usr/bin/env bash
#
# Renders on-chain block data to stdout.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Block hash (optional).

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

# Import utils.
source $NCTL/sh/utils/misc.sh

if [ "$block_hash" ]; then
    $(get_path_to_client $net) get-block \
        --node-address $(get_node_address_rpc $net $node) \
        --block-identifier $block_hash \
        | jq '.result.block'
else
    $(get_path_to_client $net) get-block \
        --node-address $(get_node_address_rpc $net $node) \
        | jq '.result.block'
fi
