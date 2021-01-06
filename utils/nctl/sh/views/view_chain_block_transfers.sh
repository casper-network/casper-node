#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Renders on-chain block transfer information.
# Arguments:
#   Block hash.
#######################################
function main()
{
    local BLOCK_HASH=${1}

    if [ "$BLOCK_HASH" ]; then
        $(get_path_to_client) get-block \
            --node-address "$(get_node_address_rpc)" \
            --block-identifier "$BLOCK_HASH" \
            | jq '.result.block'
    else
        $(get_path_to_client) get-block \
            --node-address "$(get_node_address_rpc)" \
            | jq '.result.block'
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset BLOCK_HASH

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        block) BLOCK_HASH=${VALUE} ;;
        *)
    esac
done

main "${BLOCK_HASH:-""}"
