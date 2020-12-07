#!/usr/bin/env bash
#
# Displays node status.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_DAEMON_TYPE - type of daemon service manager.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier (optional).

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

# Import daemon specific node control functions.
source $NCTL/sh/node/ctl_$NCTL_DAEMON_TYPE.sh

# Import net specific vars.
source $(get_path_to_net_vars $NET_ID)

#######################################
# Main
#######################################

# Display node status.
if [ $NODE_ID == "all" ]; then
    do_node_status_all $NET_ID $NCTL_NET_NODE_COUNT
else
    do_node_status $NET_ID $NODE_ID
fi
