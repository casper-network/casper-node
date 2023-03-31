#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Displays to stdout RPC schema.
#######################################
function main()
{
    local ENDPOINT=${1}

    if [ "$ENDPOINT" = "all" ]; then
        curl $NCTL_CURL_ARGS_FOR_NODE_RELATED_QUERIES --header 'Content-Type: application/json' \
            --request POST "$(get_node_address_rpc_for_curl)" \
            --data-raw '{
                "id": 1,
                "jsonrpc": "2.0",
                "method": "rpc.discover"
            }' | jq '.result.schema.methods[].name'
    else
        curl $NCTL_CURL_ARGS_FOR_NODE_RELATED_QUERIES --header 'Content-Type: application/json' \
            --request POST "$(get_node_address_rpc_for_curl)" \
            --data-raw '{
                "id": 1,
                "jsonrpc": "2.0",
                "method": "rpc.discover"
            }' | jq '.result.schema.methods[] | select(.name == "'"$ENDPOINT"'")'

    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset ENDPOINT

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        endpoint) ENDPOINT=${VALUE} ;;
        *)
    esac
done

main "${ENDPOINT:-"all"}"
