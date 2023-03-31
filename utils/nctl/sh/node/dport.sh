#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh


#######################################
# Connect to the diagnostics port of
# the given node
# Arguments:
#   Node ordinal identifier.
#######################################
function main()
{
    local NODE_ID=${1}
    local NODE_CONFIG_PATH=$(get_path_to_node_config $NODE_ID)
    local DPORT_SOCKET_PATH="${NODE_CONFIG_PATH}/1_0_0/debug.socket"

    socat readline "unix:${DPORT_SOCKET_PATH}"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

main "${NODE_ID:-1}"
