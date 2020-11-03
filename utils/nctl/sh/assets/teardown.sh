#!/usr/bin/env bash
#
# Tears down an entire network.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_DAEMON_TYPE - type of daemon service manager.
# Arguments:
#   Network ordinal identifer.

# Import utils.
source $NCTL/sh/utils/misc.sh

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

log "network #$net: tearing down assets ... please wait"

# Stop all spinning nodes.
source $NCTL/sh/node/stop.sh net=$net node=all

# Set daemon handler.
if [ $NCTL_DAEMON_TYPE = "supervisord" ]; then
    daemon_mgr=$NCTL/sh/daemon/supervisord/daemon_kill.sh
fi

# Kill service daemon (if appropriate).
source $daemon_mgr $net

# Delete artefacts.
rm -rf $NCTL/assets/net-$net

log "network #$net: assets torn down."
