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

state_root_hash=$(source $NCTL/sh/views/view_chain_state_root_hash.sh)
account_key=$(cat $NCTL/assets/net-$net/users/user-$user/public_key_hex)
purse_uref=$(
    source $NCTL/sh/views/view_chain_account.sh net=$net root-hash=$state_root_hash account-key=$account_key \
    | jq '.Account.main_purse' \
    | sed -e 's/^"//' -e 's/"$//'
    ) 

source $NCTL/sh/views/view_chain_account_balance.sh net=$net node=$node \
    root-hash=$state_root_hash \
    purse-uref=$purse_uref \
    typeof="user-"$user