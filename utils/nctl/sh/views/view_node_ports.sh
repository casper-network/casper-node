#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Renders ports at specified node(s).
# Arguments:
#   Node ordinal identifier.
#######################################
function main()
{
    local NODE_ID=${1}

    if [ "$NODE_ID" = "all" ]; then
        for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
        do
            echo "----------------------------------------------------------------------------------------------------------------------------------------------------------------"
            do_render "$NODE_ID"
        done
        echo "----------------------------------------------------------------------------------------------------------------------------------------------------------------"
    else
        do_render "$NODE_ID"
    fi
}

#######################################
# Displays to stdout current node ports.
# Globals:
#   NCTL_BASE_PORT_NETWORK - base port type.
# Arguments:
#   Node ordinal identifier.
#######################################
function do_render()
{
    local NODE_ID=${1}
    local PORT_VNET
    local PORT_REST
    local PORT_SSE
    local PORT_BINARY

    PORT_VNET=$(get_node_port "$NCTL_BASE_PORT_NETWORK" "$NODE_ID")
    PORT_REST=$(get_node_port_rest "$NODE_ID")
    PORT_SSE=$(get_node_port_sse "$NODE_ID")
    PORT_BINARY=$(get_node_port_binary "$NODE_ID")

    log "node-$NODE_ID :: VNET @ $PORT_VNET :: REST @ $PORT_REST :: SSE @ $PORT_SSE :: BINARY @ $PORT_BINARY"
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

main "${NODE_ID:-"all"}"
