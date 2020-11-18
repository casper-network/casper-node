#!/usr/bin/env bash
#
# Displays node status.
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

# Set daemon handler.
if [ $NCTL_DAEMON_TYPE = "supervisord" ]; then
    daemon_mgr=$NCTL/sh/daemon/supervisord/node_status.sh
fi

# Invoke daemon handler.
source $daemon_mgr $net
