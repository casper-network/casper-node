#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Submits an auction withdrawal.
# Arguments:
#   Validator ordinal identifier.
#   Withdrawal amount.
#   Flag indicating whether to emit log messages.
#######################################
function main()
{
    local BIDDER_ID=${1}
    local AMOUNT=${2}
    local QUIET=${3:-"FALSE"}
    local CHAIN_NAME
    local GAS_PRICE
    local GAS_PAYMENT
    local NODE_ADDRESS
    local PATH_TO_CLIENT
    local PATH_TO_CONTRACT
    local BIDDER_SECRET_KEY
    local BIDDER_ACCOUNT_KEY
    local BIDDER_MAIN_PURSE_UREF

    CHAIN_NAME=$(get_chain_name)
    GAS_PRICE=${GAS_PRICE:-$NCTL_DEFAULT_GAS_PRICE}
    GAS_PAYMENT=${GAS_PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
    NODE_ADDRESS=$(get_node_address_rpc)
    PATH_TO_CLIENT=$(get_path_to_client)
    PATH_TO_CONTRACT=$(get_path_to_contract "auction/withdraw_bid.wasm")

    BIDDER_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_NODE" "$BIDDER_ID")
    BIDDER_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_NODE" "$BIDDER_ID" | tr '[:upper:]' '[:lower:]')
    BIDDER_MAIN_PURSE_UREF=$(get_main_purse_uref "$BIDDER_ACCOUNT_KEY" | tr '[:upper:]' '[:lower:]')

    if [ "$QUIET" != "TRUE" ]; then
        log "dispatching deploy -> withdraw_bid.wasm"
        log "... chain = $CHAIN_NAME"
        log "... dispatch node = $NODE_ADDRESS"
        log "... contract = $PATH_TO_CONTRACT"
        log "... bidder id = $BIDDER_ID"
        log "... bidder account key = $BIDDER_ACCOUNT_KEY"
        log "... bidder secret key = $BIDDER_SECRET_KEY"
        log "... bidder main purse uref = $BIDDER_MAIN_PURSE_UREF"
        log "... withdrawal amount = $AMOUNT"
    fi

    DEPLOY_HASH=$(
        $PATH_TO_CLIENT put-deploy \
            --chain-name "$CHAIN_NAME" \
            --gas-price "$GAS_PRICE" \
            --node-address "$NODE_ADDRESS" \
            --payment-amount "$GAS_PAYMENT" \
            --ttl "1day" \
            --secret-key "$BIDDER_SECRET_KEY" \
            --session-arg "$(get_cl_arg_account_key 'public_key' "$BIDDER_ACCOUNT_KEY")" \
            --session-arg "$(get_cl_arg_u512 'amount' "$AMOUNT")" \
            --session-arg "$(get_cl_arg_opt_uref 'unbond_purse' "$BIDDER_MAIN_PURSE_UREF")" \
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

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

main "${NODE_ID:-1}" \
     "${AMOUNT:-$(get_node_staking_weight "${NODE_ID:-1}")}"
