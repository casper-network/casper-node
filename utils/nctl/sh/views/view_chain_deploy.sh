#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Renders on-chain deploy information.
# Arguments:
#   Deploy hash.
#######################################
function main()
{
    local DEPLOY_HASH=${1}

    $(get_path_to_client) get-deploy \
        --node-address "$(get_node_address_rpc)" \
        "$DEPLOY_HASH" \
        | jq '.result'
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        deploy) DEPLOY_HASH=${VALUE} ;;
        *)
    esac
done

main "$DEPLOY_HASH"
