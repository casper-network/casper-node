#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Renders on-chain auction information.
#######################################
function main()
{
    $(get_path_to_client) get-auction-info \
        --node-address "$(get_node_address_rpc)" \
        | jq '.result'
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

main
