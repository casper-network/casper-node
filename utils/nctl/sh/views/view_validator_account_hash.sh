#!/usr/bin/env bash
#
# Renders a validator's account hash.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   User ordinal identifier.

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Displays to stdout a validator's account hash.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   User ordinal identifier.
#######################################
function _view_validator_account_hash() {
    account_key=$(cat $NCTL/assets/net-$1/nodes/node-$2/keys/public_key_hex)
    log "account hash :: net-$1:validator-$2 -> "$(get_account_hash $account_key)
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
        _view_validator_account_hash $net $node_idx
    done
else
    _view_validator_account_hash $net $node
fi
