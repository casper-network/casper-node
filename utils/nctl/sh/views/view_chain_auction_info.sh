#!/usr/bin/env bash

source $NCTL/sh/utils/main.sh

#######################################
# Renders on-chain auction information.
#######################################
function main()
{
    local NODE_ADDRESS=$(get_node_address_rpc)

    $(get_path_to_client) get-auction-info \
        --node-address $NODE_ADDRESS \
        | jq '.result'
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

main
