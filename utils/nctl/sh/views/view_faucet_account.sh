#!/usr/bin/env bash
#
# Renders account information to stdout.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Chain root state hash.
#   Account key.

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset account_key
unset net
unset node
unset state_root_hash

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        account-key) account_key=${VALUE} ;;
        root-hash) state_root_hash=${VALUE} ;;
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

source $NCTL/sh/views/view_chain_account.sh net=$net node=$node \
    root-hash=$(source $NCTL/sh/views/view_chain_state_root_hash.sh) \
    account-key=$(cat $NCTL/assets/net-$net/faucet/public_key_hex)
