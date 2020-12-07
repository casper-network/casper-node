#!/usr/bin/env bash
#
# Displays node logs.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Log type.

#######################################
# Destructure input args.
#######################################

# Unset to avoid parameter collisions.
unset NET_ID
unset NODE_ID
unset LOG_TYPE

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        typeof) LOG_TYPE=${VALUE} ;;
        *)
    esac
done

# Set defaults.
NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-1}
LOG_TYPE=${LOG_TYPE:-stdout}

#######################################
# Main
#######################################

# Import utils.
source $NCTL/sh/utils.sh

# View log via less.
less $(get_path_to_node $NET_ID $NODE_ID)/logs/$LOG_TYPE.log
