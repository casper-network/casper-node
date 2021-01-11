#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Spins up a node in interactive mode.
# Arguments:
#   Node ordinal identifier.
#   Node software logging level.
#######################################
function main() 
{
    local NODE_ID=${1}
    local LOG_LEVEL=${2}
    local PATH_NODE_CONFIG

    export RUST_LOG=$LOG_LEVEL

    PATH_NODE_CONFIG=$(get_path_to_node_config "$NODE_ID")
    "$(get_path_to_net)"/bin/casper-node validator "$PATH_NODE_CONFIG" ;
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset LOG_LEVEL
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        loglevel) LOG_LEVEL=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

LOG_LEVEL=${LOG_LEVEL:-$RUST_LOG}
LOG_LEVEL=${LOG_LEVEL:-debug}
NODE_ID=${NODE_ID:-1}

main "$NODE_ID" "$LOG_LEVEL"
