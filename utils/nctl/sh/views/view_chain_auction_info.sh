#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Renders on-chain auction information.
# Arguments:
#   Block height.
#######################################
function main()
{
    local BLOCK_HEIGHT=${1}

    if [ "$BLOCK_HEIGHT" ]; then
        curl $NCTL_CURL_ARGS_FOR_NODE_RELATED_QUERIES --header 'Content-Type: application/json' \
            --request POST "$(get_node_address_rpc_for_curl)" \
            --data-raw "{
                \"id\": 1,
                \"jsonrpc\": \"2.0\",
                \"method\": \"state_get_auction_info\",
                \"params\": { \"block_identifier\": { \"Height\": $BLOCK_HEIGHT } }
            }" | jq '.result'
    else
        curl $NCTL_CURL_ARGS_FOR_NODE_RELATED_QUERIES --header 'Content-Type: application/json' \
            --request POST "$(get_node_address_rpc_for_curl)" \
            --data-raw '{
                "id": 1,
                "jsonrpc": "2.0",
                "method": "state_get_auction_info"
            }' | jq '.result'
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset BLOCK_HEIGHT

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        block-height) BLOCK_HEIGHT=${VALUE} ;;
        *)
    esac
done

main "$BLOCK_HEIGHT"
