#!/usr/bin/env bash
#
# Stops up a node within a network.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_DAEMON_TYPE - type of node daemon to run.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset NET_ID
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

# Set defaults.
NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-"all"}

#######################################
# Imports
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Import daemon specific node control functions.
source $NCTL/sh/node/ctl_$NCTL_DAEMON_TYPE.sh

# Import net specific vars.
source $(get_path_to_net_vars $NET_ID)

#######################################
# Main
#######################################

# Stop node(s).
if [ $NODE_ID == "all" ]; then
    do_node_stop_all $NET_ID $NCTL_NET_NODE_COUNT $NCTL_NET_BOOTSTRAP_COUNT
else
    log "net-$NET_ID: stopping node ... "
    do_node_stop $NET_ID $NODE_ID
fi

# Display status.
sleep 1.0
source $NCTL/sh/node/status.sh net=$NET_ID node=$NODE_ID
