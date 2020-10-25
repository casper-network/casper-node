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
# Displays to stdout a validator's account balance.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifer.
#   Node ordinal identifer.
#######################################
function _view_validator_account_balance() {
    state_root_hash=$(source $NCTL/sh/views/view_chain_state_root_hash.sh)
    account_key=$(cat $NCTL/assets/net-$1/nodes/node-$2/keys/public_key_hex)
    purse_uref=$(
        source $NCTL/sh/views/view_chain_account.sh net=$1 root-hash=$state_root_hash account-key=$account_key \
        | jq '.Account.main_purse' \
        | sed -e 's/^"//' -e 's/"$//'
        ) 
    balance=$(
        $NCTL/assets/net-$1/bin/casper-client get-balance \
            --node-address $(get_node_address $1 $2) \
            --state-root-hash $state_root_hash \
            --purse-uref $purse_uref \
            | jq '.result.balance_value' \
            | sed -e 's/^"//' -e 's/"$//'
        )
    log "net-$1 :: validator $2 :: balance = "$balance
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
