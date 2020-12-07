#!/usr/bin/env bash
#
# Tears down an entire network.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_DAEMON_TYPE - type of daemon service manager.
# Arguments:
#   Network ordinal identifier.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset NET_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        *)
    esac
done

# Set defaults.
NET_ID=${NET_ID:-1}

#######################################
# Imports
#######################################

# Import utils.
source $NCTL/sh/utils.sh

#######################################
# Main
#######################################

log "net-$NET_ID: tearing down assets ... please wait"

# Stop all spinning nodes.
source $NCTL/sh/node/stop.sh net=$NET_ID node=all

# If supervisord then kill.
if [ $NCTL_DAEMON_TYPE = "supervisord" ]; then
    do_supervisord_kill $NET_ID
fi

# Delete artefacts.
rm -rf $NCTL/assets/net-$NET_ID

log "net-$NET_ID: assets torn down."
