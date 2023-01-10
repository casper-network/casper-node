#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Renders status at specified node(s).
# Arguments:
#   Node ordinal identifier.
#######################################
function main()
{
    local NODE_ID=${1}

    if [ "$NODE_ID" = "all" ]; then
        for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
        do
            if [ "$(get_node_is_up "$NODE_ID")" = true ]; then
                echo "------------------------------------------------------------------------------------------------------------------------------------"
                do_render "$NODE_ID"
            fi        
        done
        echo "------------------------------------------------------------------------------------------------------------------------------------"
    else
        if [ "$(get_node_is_up "$NODE_ID")" = true ]; then
            do_render "$NODE_ID"
        fi        
    fi
}

#######################################
# Displays to stdout current node status.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function do_render()
{
    local NODE_ID=${1}
    local NODE_ADDRESS_CURL
    local NODE_API_RESPONSE
    
    NODE_ADDRESS_CURL=$(get_node_address_rpc_for_curl "$NODE_ID")
    NODE_API_RESPONSE=$(
        curl $NCTL_CURL_ARGS_FOR_NODE_RELATED_QUERIES --header 'Content-Type: application/json' \
            --request POST "$NODE_ADDRESS_CURL" \
            --data-raw '{
                "id": 1,
                "jsonrpc": "2.0",
                "method": "info_get_status"
            }' | jq '.result'
    )

    if [ -z "$NODE_API_RESPONSE" ]; then
        log "node #$NODE_ID :: status: N/A"
    else
        log "node #$NODE_ID :: status:"
        echo "$NODE_API_RESPONSE" | jq '.'
    fi
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
