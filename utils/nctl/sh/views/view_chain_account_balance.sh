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

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset net
unset node
unset purse_uref
unset state_root_hash
unset prefix

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        purse-uref) purse_uref=${VALUE} ;;
        root-hash) state_root_hash=${VALUE} ;;
        prefix) prefix=${VALUE} ;;
        *)
    esac
done

# Set defaults.
net=${net:-1}
node=${node:-1}
prefix=${prefix:-"account"}
state_root_hash=${state_root_hash:-$(get_state_root_hash $net $node)}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils/misc.sh

balance=$(
    $(get_path_to_client $net) get-balance \
        --node-address $(get_node_address_rpc $net $node) \
        --state-root-hash $state_root_hash \
        --purse-uref $purse_uref \
        | jq '.result.balance_value' \
        | sed -e 's/^"//' -e 's/"$//'
    )
log $prefix" balance = "$balance
