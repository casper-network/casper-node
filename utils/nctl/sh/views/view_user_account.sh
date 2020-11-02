#!/usr/bin/env bash
#
# Renders a network faucet account key.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.

# Import utils.
source $NCTL/sh/utils/misc.sh
source $NCTL/sh/utils/queries.sh

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset net
unset node
unset user

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) net=${VALUE} ;;
        node) node=${VALUE} ;;
        user) user=${VALUE} ;;
        *)
    esac
done

# Set defaults.
net=${net:-1}
node=${node:-1}
user=${user:-1}

#######################################
# Main
#######################################

state_root_hash=$(get_state_root_hash $net $node)
account_key=$(cat $NCTL/assets/net-$net/users/user-$user/public_key_hex)
source $NCTL/sh/views/view_chain_account.sh net=$net node=$node \
    root-hash=$state_root_hash \
    account-key=$account_key
