#!/usr/bin/env bash
#
# Renders a user's account hash.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   User ordinal identifier.

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
user=${user:-"all"}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Import net vars.
source $(get_path_to_net_vars $net)

# Render user main purse u-ref.
if [ $user = "all" ]; then
    for IDX in $(seq 1 $NCTL_NET_USER_COUNT); 
    do
        render_account_main_purse_uref $net $node $NCTL_ACCOUNT_TYPE_USER $IDX
    done
else
    render_account_main_purse_uref $net $node $NCTL_ACCOUNT_TYPE_USER $user
fi
