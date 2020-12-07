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
unset BLOCK_HASH
unset NET_ID
unset NODE_ID

# Destructure named args.
for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        block) BLOCK_HASH=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

# Set defaults.
NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-1}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Render on-chain block information.
render_chain_block $NET_ID $NODE_ID $BLOCK_HASH
