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

NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-1}

#######################################
# Imports
#######################################

source $NCTL/sh/utils.sh

#######################################
# Main
#######################################

# View log via less.
less $(get_path_to_node $NET_ID $NODE_ID)/logs/stderr.log
