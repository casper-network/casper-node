#!/usr/bin/env bash

source $NCTL/sh/utils/main.sh

#######################################
# Displays to stdout RPC schema.
#######################################
function main()
{
    local NODE_ADDRESS_CURL=$(get_node_address_rpc_for_curl)

    curl -s --header 'Content-Type: application/json' \
        --request POST $NODE_ADDRESS_CURL \
        --data-raw '{
            "id": 1,
            "jsonrpc": "2.0",
            "method": "rpc.discover"
        }' | jq '.result'
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

main
