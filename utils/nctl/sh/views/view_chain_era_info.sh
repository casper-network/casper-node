#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Renders on-chain era information.
# Arguments:
#   Node ordinal identifier (optional).
#######################################
function main()
{
    local NODE_ID=${1}

    $(get_path_to_client) get-era-info-by-switch-block \
        --node-address "$(get_node_address_rpc "$NODE_ID")" \
        --block-identifier "" \
        | jq '.result'
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

main "$NODE_ID"
