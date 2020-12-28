#!/usr/bin/env bash

#######################################
# Dispatches previously prepared transfers to a test net.
# Arguments:
#   Batch ordinal identifier.
#   Batch type.
#   Transfer dispatch interval.
#   Node ordinal identifier.
#######################################
function do_dispatch_batch()
{
    local BATCH_ID=${1}
    local BATCH_TYPE=${2}
    local INTERVAL=${3}
    local NODE_ID=${4}

    local DEPLOY_ID
    local DISPATCH_NODE_ADDRESS
    local NODE_ADDRESS
    local PATH_TO_BATCH
    local PATH_TO_CLIENT
    local PATH_TO_DEPLOY


    # Set node address.
    if [ "$NODE_ID" == "random" ]; then
        unset NODE_ADDRESS
    elif [ "$NODE_ID" -eq 0 ]; then
        NODE_ADDRESS=$(get_node_address_rpc)
    else
        NODE_ADDRESS=$(get_node_address_rpc "$NODE_ID")
    fi

    # Dispatch deploy batch.
    PATH_TO_BATCH=$(get_path_to_net)/deploys/$BATCH_TYPE/batch-$BATCH_ID
    if [ ! -d "$PATH_TO_BATCH" ]; then
        log "ERROR: no batch exists on file system - have you prepared it ?"
    else
        DEPLOY_ID=0
        PATH_TO_CLIENT=$(get_path_to_client)
        for USER_ID in $(seq 1 "$(get_count_of_users)")
        do
            for TRANSFER_ID in $(seq 1 100000)
            do
                PATH_TO_DEPLOY=$PATH_TO_BATCH/user-$USER_ID/transfer-$TRANSFER_ID.json
                if [ ! -f "$PATH_TO_DEPLOY" ]; then
                    break
                else
                    DEPLOY_ID=$((DEPLOY_ID + 1)) 
                    DISPATCH_NODE_ADDRESS=${NODE_ADDRESS:-$(get_node_address_rpc)}
                    DEPLOY_HASH=$(
                        $PATH_TO_CLIENT send-deploy \
                            --node-address "$DISPATCH_NODE_ADDRESS" \
                            --input "$PATH_TO_DEPLOY" \
                            | jq '.result.deploy_hash' \
                            | sed -e 's/^"//' -e 's/"$//'                                
                    )
                    log "deploy #$DEPLOY_ID :: batch #$BATCH_ID :: user #$USER_ID :: $DEPLOY_HASH :: $DISPATCH_NODE_ADDRESS"
                    sleep "$INTERVAL"
                fi
            done
        done
    fi
}
