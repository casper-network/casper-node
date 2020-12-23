#!/usr/bin/env bash

source $NCTL/sh/utils.sh

#######################################
# Dispatches native transfers to a test net.
# Arguments:
#   Transfer amount.
#   User ordinal identifier.
#   Count of transfers to be dispatched.
#   Transfer dispatch interval.
#   Node ordinal identifier.
#######################################
function main()
{
    local AMOUNT=${1}
    local USER_ID=${2}
    local TRANSFERS=${3}
    local INTERVAL=${4}
    local NODE_ID=${5}

    local CP1_SECRET_KEY=$(get_path_to_secret_key $NCTL_ACCOUNT_TYPE_FAUCET)
    local CP1_PUBLIC_KEY=$(get_account_key $NCTL_ACCOUNT_TYPE_FAUCET)
    local CP2_PUBLIC_KEY=$(get_account_key $NCTL_ACCOUNT_TYPE_USER $USER_ID)

    local CHAIN_NAME=$(get_chain_name)
    local GAS_PRICE=${GAS_PRICE:-$NCTL_DEFAULT_GAS_PRICE}
    local GAS_PAYMENT=${GAS_PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
    local PATH_TO_CLIENT=$(get_path_to_client)

    if [ $NODE_ID == "random" ]; then
        unset NODE_ADDRESS
    elif [ $NODE_ID -eq 0 ]; then
        local NODE_ADDRESS=$(get_node_address_rpc)
    else
        local NODE_ADDRESS=$(get_node_address_rpc $NODE_ID)
    fi

    log "dispatching $TRANSFERS native transfers"
    log "... chain=$CHAIN_NAME"
    log "... transfer amount=$AMOUNT"
    log "... transfer interval=$INTERVAL (s)"
    log "... counter-party 1 public key=$CP1_PUBLIC_KEY"
    log "... counter-party 2 public key=$CP2_PUBLIC_KEY"
    log "... dispatched deploys:"

    local DISPATCHED=0
    while [ $DISPATCHED -lt $TRANSFERS ];
    do
        local DISPATCH_NODE_ADDRESS=${NODE_ADDRESS:-$(get_node_address_rpc)}
        DEPLOY_HASH=$(
            $PATH_TO_CLIENT transfer \
                --chain-name $CHAIN_NAME \
                --gas-price $GAS_PRICE \
                --node-address $DISPATCH_NODE_ADDRESS \
                --payment-amount $GAS_PAYMENT \
                --ttl "1day" \
                --secret-key $CP1_SECRET_KEY \
                --amount $AMOUNT \
                --target-account $CP2_PUBLIC_KEY \
                | jq '.result.deploy_hash' \
                | sed -e 's/^"//' -e 's/"$//'
            )
        DISPATCHED=$((DISPATCHED + 1))
        log "... #$DISPATCHED :: $DISPATCH_NODE_ADDRESS :: $DEPLOY_HASH"
        sleep $INTERVAL
    done

    log "dispatched $TRANSFERS native transfers"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset AMOUNT
unset INTERVAL
unset NODE_ID
unset TRANSFERS
unset USER_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        interval) INTERVAL=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;        
        transfers) TRANSFERS=${VALUE} ;;
        user) USER_ID=${VALUE} ;;
        *)
    esac
done


main \
    ${AMOUNT:-$NCTL_DEFAULT_TRANSFER_AMOUNT} \
    ${USER_ID:-1} \
    ${TRANSFERS:-100} \
    ${INTERVAL:-0.01} \
    ${NODE_ID:-"random"}

