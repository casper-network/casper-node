#!/usr/bin/env bash
#
# Renders account balance to stdout.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Chain root state hash.
#   Account purse uref.

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset net
unset node
unset purse_uref
unset state_root_hash
unset typeof

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        purse-uref) purse_uref=${VALUE} ;;
        root-hash) state_root_hash=${VALUE} ;;
        typeof) typeof=${VALUE} ;;
        *)
    esac
done

# Set defaults.
net=${net:-1}
node=${node:-1}
typeof=${typeof:-"account"}

#######################################
# Main
#######################################

balance=$(
    $NCTL/assets/net-$net/bin/casper-client get-balance \
        --node-address $(get_node_address $net $node) \
        --state-root-hash $state_root_hash \
        --purse-uref $purse_uref \
        | jq '.result.balance_value' \
        | sed -e 's/^"//' -e 's/"$//'
    )
log $typeof" balance = "$balance
