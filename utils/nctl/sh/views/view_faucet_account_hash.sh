#!/usr/bin/env bash
#
# Renders a faucet's account hash.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset net

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) net=${VALUE} ;;
        *)
    esac
done

# Set defaults.
net=${net:-1}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils/misc.sh

# Render account hash.
render_account_hash $net $NCTL_ACCOUNT_TYPE_FAUCET
