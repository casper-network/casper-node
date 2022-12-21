#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Submits an auction bid.
# Arguments:
#   Bidder ordinal identifier.
#   Bid amount.
#   Delegation rate.
#   Flag indicating whether to emit log messages.
#######################################
function main()
{
    local BIDDER_ID=${1}
    local BID_AMOUNT=${2}
    local BID_DELEGATION_RATE=${3}
    local QUIET=${4:-"FALSE"}

    local CHAIN_NAME
    local GAS_PAYMENT
    local NODE_ADDRESS
    local PATH_TO_CLIENT
    local PATH_TO_CONTRACT
    local BIDDER_ACCOUNT_KEY
    local BIDDER_SECRET_KEY

    CHAIN_NAME=$(get_chain_name)
    GAS_PAYMENT=${GAS_PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
    NODE_ADDRESS=$(get_node_address_rpc)
    PATH_TO_CLIENT=$(get_path_to_client)
    PATH_TO_CONTRACT=$(get_path_to_contract "auction/add_bid.wasm")

    BIDDER_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_NODE" "$BIDDER_ID" | tr '[:upper:]' '[:lower:]')
    BIDDER_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_NODE" "$BIDDER_ID")

    if [ "$QUIET" != "TRUE" ]; then
        log "dispatching deploy -> add_bid.wasm"
        log "... chain = $CHAIN_NAME"
        log "... dispatch node = $NODE_ADDRESS"
        log "... contract = $PATH_TO_CONTRACT"
        log "... bidder id = $BIDDER_ID"
        log "... bidder secret key = $BIDDER_SECRET_KEY"
        log "... bid amount = $BID_AMOUNT"
        log "... bid delegation rate = $BID_DELEGATION_RATE"
    fi

    DEPLOY_HASH=$(
        $PATH_TO_CLIENT put-deploy \
            --chain-name "$CHAIN_NAME" \
            --node-address "$NODE_ADDRESS" \
            --payment-amount "$GAS_PAYMENT" \
            --ttl "5min" \
            --secret-key "$BIDDER_SECRET_KEY" \
            --session-arg "$(get_cl_arg_account_key 'public_key' "$BIDDER_ACCOUNT_KEY")" \
            --session-arg "$(get_cl_arg_u512 'amount' "$BID_AMOUNT")" \
            --session-arg "$(get_cl_arg_u8 'delegation_rate' "$BID_DELEGATION_RATE")" \
            --session-path "$PATH_TO_CONTRACT" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )

    if [ "$QUIET" != "TRUE" ]; then
        log "deploy dispatched:"
        log "... deploy hash = $DEPLOY_HASH"
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset AMOUNT
unset NODE_ID
unset DELEGATION_RATE
unset QUIET

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        rate) DELEGATION_RATE=${VALUE} ;;
        quiet) QUIET=${VALUE} ;;
        *)
    esac
done

main "${NODE_ID:-6}" \
     "${AMOUNT:-$(get_node_staking_weight "${NODE_ID:-6}")}" \
     "${DELEGATION_RATE:-6}" \
     ${QUIET:-"FALSE"}
