#!/usr/bin/env bash
#
# Resets node logs.
# Globals:
#   NCTL - path to nctl home directory.
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

# Import net vars.
source $(get_path_to_net_vars $NET_ID)

#######################################
# Main
#######################################

# Reset logs.
if [ $NODE_ID = "all" ]; then
    for IDX in $(seq 1 $NCTL_NET_NODE_COUNT)
    do
        rm $(get_path_to_node $NET_ID $IDX)/logs/*.log > /dev/null 2>&1
    done
else
    rm $(get_path_to_node $NET_ID $NODE_ID)/logs/*.log > /dev/null 2>&1
fi
