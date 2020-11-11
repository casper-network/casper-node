#!/usr/bin/env bash
#
# Renders a user account key.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   User ordinal identifier.

# Import utils.
source $NCTL/sh/utils/misc.sh

#######################################
# Displays to stdout a user's account key.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   User ordinal identifier.
#######################################
function _view_user_account_key() {
    declare path_key=$NCTL/assets/net-$1/users/user-$2/public_key_hex
    log "account key :: net-$1:user-$2 -> "$(cat $path_key)
}

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

if [ $user = "all" ]; then
    source $NCTL/assets/net-$net/vars
    for user_idx in $(seq 1 $NCTL_NET_USER_COUNT)
    do
        _view_user_account_key $net $user_idx
    done
else
    _view_user_account_key $net $user
fi
