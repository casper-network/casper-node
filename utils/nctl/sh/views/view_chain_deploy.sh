#!/usr/bin/env bash

source $NCTL/sh/utils/main.sh

#######################################
# Renders on-chain deploy information.
# Arguments:
#   Deploy hash.
#######################################
function main()
{
    local DEPLOY_HASH=${1}
    local NODE_ADDRESS=$(get_node_address_rpc)

    $(get_path_to_client) get-deploy \
        --node-address $NODE_ADDRESS \
        $DEPLOY_HASH \
        | jq '.result'
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        deploy) DEPLOY_HASH=${VALUE} ;;
        *)
    esac
done

main $DEPLOY_HASH
