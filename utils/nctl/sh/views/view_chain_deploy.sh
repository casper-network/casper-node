#!/usr/bin/env bash
#
# Renders on-chain deploy data to stdout.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Deploy hash.

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

# Import utils.
source $NCTL/sh/utils.sh

# Render on-chain deploy information.
render_chain_deploy $net $node $deploy_hash
