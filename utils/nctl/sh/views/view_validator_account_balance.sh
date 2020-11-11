#!/usr/bin/env bash
#
# Renders a validators's account balance.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.

# Import utils.
source $NCTL/sh/utils/misc.sh
source $NCTL/sh/utils/queries.sh

#######################################
# Displays to stdout a validator's account balance.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function _view_validator_account_balance() {
    state_root_hash=$(get_state_root_hash $1 $2)
    account_key=$(cat $NCTL/assets/net-$1/nodes/node-$2/keys/public_key_hex)
    purse_uref=$(get_main_purse_uref $1 $state_root_hash $account_key)
    source $NCTL/sh/views/view_chain_account_balance.sh net=$1 node=$2 \
        root-hash=$state_root_hash \
        purse-uref=$purse_uref \
        typeof="validator-"$2
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

if [ $node = "all" ]; then
    source $NCTL/assets/net-$net/vars
    for node_idx in $(seq 1 $NCTL_NET_NODE_COUNT)
    do
        _view_validator_account_balance $net $node_idx
    done
else
    _view_validator_account_balance $net $node
fi
