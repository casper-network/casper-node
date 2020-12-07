#!/usr/bin/env bash
#
# Renders a user's account balance.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset NET_ID
unset NODE_ID
unset USER_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        user) USER_ID=${VALUE} ;;
        *)
    esac
done

# Set defaults.
NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-1}
USER_ID=${USER_ID:-"all"}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Import net vars.
source $(get_path_to_net_vars $NET_ID)

# Render account balance(s).
if [ $USER_ID = "all" ]; then
    for IDX in $(seq 1 $NCTL_NET_USER_COUNT)
    do
        render_account_balance $NET_ID $NODE_ID $NCTL_ACCOUNT_TYPE_USER $IDX
    done
else
    render_account_balance $NET_ID $NODE_ID $NCTL_ACCOUNT_TYPE_USER $USER_ID
fi
