#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Displays to stdout RPC schema.
#######################################
function main()
{
    curl -s --header 'Content-Type: application/json' \
        --request POST "$(get_node_address_rpc_for_curl)" \
        --data-raw '{
            "id": 1,
            "jsonrpc": "2.0",
            "method": "rpc.discover"
        }' | jq '.result.schema'
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

main
