#!/usr/bin/env bash
#
# Starts up a node within a network.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_DAEMON_TYPE - type of daemon service manager.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset LOG_LEVEL
unset NET_ID
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        loglevel) LOG_LEVEL=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

# Set defaults.
LOG_LEVEL=${LOG_LEVEL:-$RUST_LOG}
LOG_LEVEL=${LOG_LEVEL:-debug}
NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-"all"}

# Set rust log level.
export RUST_LOG=$LOG_LEVEL

#######################################
# Imports
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# Import net specific vars.
source $(get_path_to_net_vars $NET_ID)

# Import daemon specific node control functions.
source $NCTL/sh/node/ctl_$NCTL_DAEMON_TYPE.sh

#######################################
# Main
#######################################

# Start node(s).
if [ $NODE_ID == "all" ]; then
    do_node_start_all $NET_ID $NCTL_NET_NODE_COUNT $NCTL_NET_BOOTSTRAP_COUNT
else
    log "starting node :: net-$NET_ID.node-$NODE_ID"
    do_node_start $NET_ID $NODE_ID
fi

# Display status.
sleep 1.0
source $NCTL/sh/node/status.sh net=$NET_ID node=$NODE_ID
