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
unset user

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) net=${VALUE} ;;
        user) user=${VALUE} ;;
        *)
    esac
done

# Set defaults.
net=${net:-1}
user=${user:-"all"}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils/misc.sh

# Import vars.
source $(get_path_to_net_vars $net)

# Render user account hash.
if [ $user = "all" ]; then
    for idx in $(seq 1 $NCTL_NET_USER_COUNT)
    do
        render_account_hash $net $NCTL_ACCOUNT_TYPE_USER $idx
    done
else
    render_account_hash $net $NCTL_ACCOUNT_TYPE_USER $user
fi
