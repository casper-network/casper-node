#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Dispatches native transfers to a test net.
# Arguments:
#   Transfer amount.
#   User ordinal identifier.
#   Count of transfers to be dispatched.
#   Transfer dispatch interval.
#   Node ordinal identifier.
#   Verbosity flag.
#######################################
function main()
{
    local AMOUNT=${1}
    local USER_ID=${2}
    local TRANSFERS=${3}
    local INTERVAL=${4}
    local NODE_ID=${5}
    local VERBOSE=${6}

    local CHAIN_NAME
    local GAS_PAYMENT
    local NODE_ADDRESS
    local PATH_TO_CLIENT
    local CP1_SECRET_KEY
    local CP1_ACCOUNT_KEY
    local CP2_ACCOUNT_KEY
    local DISPATCHED
    local DISPATCH_NODE_ADDRESS

    CHAIN_NAME=$(get_chain_name)
    GAS_PAYMENT=${GAS_PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
    PATH_TO_CLIENT=$(get_path_to_client)

    if [ "$NODE_ID" == "random" ]; then
        unset NODE_ADDRESS
    elif [ "$NODE_ID" -eq 0 ]; then
        NODE_ADDRESS=$(get_node_address_rpc)
    else
        NODE_ADDRESS=$(get_node_address_rpc "$NODE_ID")
    fi

    CP1_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_FAUCET")
    CP1_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_FAUCET")
    CP2_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_USER" "$USER_ID")

    if [ $VERBOSE == true ]; then
        log "dispatching $TRANSFERS native transfers"
        log "... chain=$CHAIN_NAME"
        log "... transfer amount=$AMOUNT"
        log "... transfer interval=$INTERVAL (s)"
        log "... counter-party 1 public key=$CP1_ACCOUNT_KEY"
        log "... counter-party 2 public key=$CP2_ACCOUNT_KEY"
        log "... dispatched deploys:"    
    fi

    DISPATCHED=0
    while [ $DISPATCHED -lt "$TRANSFERS" ];
    do
        DISPATCH_NODE_ADDRESS=${NODE_ADDRESS:-$(get_node_address_rpc)}
        DEPLOY_HASH=$(
            $PATH_TO_CLIENT transfer \
                --chain-name "$CHAIN_NAME" \
                --node-address "$DISPATCH_NODE_ADDRESS" \
                --payment-amount "$GAS_PAYMENT" \
                --ttl "1day" \
                --secret-key "$CP1_SECRET_KEY" \
                --amount "$AMOUNT" \
                --target-account "$CP2_ACCOUNT_KEY" \
                --transfer-id $((DISPATCHED + 1)) \
                | jq '.result.deploy_hash' \
                | sed -e 's/^"//' -e 's/"$//'
            )
        DISPATCHED=$((DISPATCHED + 1))
        if [ $VERBOSE == true ]; then
            log "... #$DISPATCHED :: $DISPATCH_NODE_ADDRESS :: $DEPLOY_HASH"
        fi
        sleep "$INTERVAL"
    done

    if [ $VERBOSE == true ]; then
        log "dispatched $TRANSFERS native transfers"
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset AMOUNT
unset INTERVAL
unset NODE_ID
unset TRANSFERS
unset USER_ID
unset VERBOSE

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        interval) INTERVAL=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;        
        transfers) TRANSFERS=${VALUE} ;;
        user) USER_ID=${VALUE} ;;
        verbose) VERBOSE=${VALUE} ;;
        *)
    esac
done

main "${AMOUNT:-$NCTL_DEFAULT_TRANSFER_AMOUNT}" \
     "${USER_ID:-1}" \
     "${TRANSFERS:-100}" \
     "${INTERVAL:-0.01}" \
     "${NODE_ID:-"random"}" \
     ${VERBOSE:-true}
