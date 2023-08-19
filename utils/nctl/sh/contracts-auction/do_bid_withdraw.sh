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
    local GAS_PAYMENT
    local NODE_ADDRESS
    local PATH_TO_CLIENT
    local PATH_TO_CONTRACT
    local BIDDER_SECRET_KEY
    local BIDDER_ACCOUNT_KEY
    local BIDDER_MAIN_PURSE_UREF
    local DEPLOY_RESULT

    log "getting chain name"
    CHAIN_NAME=$(get_chain_name)
    log "getting gas payment"
    GAS_PAYMENT=${GAS_PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
    log "getting node address"
    NODE_ADDRESS=$(get_node_address_rpc)
    log "getting path to client"
    PATH_TO_CLIENT=$(get_path_to_client)
    log "getting path to contract"
    PATH_TO_CONTRACT=$(get_path_to_contract "auction/withdraw_bid.wasm")

    log "getting secret key"
    BIDDER_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_NODE" "$BIDDER_ID")
    log "getting account key"
    BIDDER_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_NODE" "$BIDDER_ID" | tr '[:upper:]' '[:lower:]')
    log "getting main purse uref"
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

    log "... sending deploy"
    DEPLOY_RESULT =$($PATH_TO_CLIENT put-deploy \
                                 --chain-name "$CHAIN_NAME" \
                                 --node-address "$NODE_ADDRESS" \
                                 --payment-amount "$GAS_PAYMENT" \
                                 --ttl "5min" \
                                 --secret-key "$BIDDER_SECRET_KEY" \
                                 --session-arg "$(get_cl_arg_account_key 'public_key' "$BIDDER_ACCOUNT_KEY")" \
                                 --session-arg "$(get_cl_arg_u512 'amount' "$AMOUNT")" \
                                 --session-arg "$(get_cl_arg_opt_uref 'unbond_purse' "$BIDDER_MAIN_PURSE_UREF")" \
                                 --session-path "$PATH_TO_CONTRACT")
    if [ ! -z "DEPLOY_RESULT" ]; then
        log "failed to get deploy result"
        log "... node address: $NODE_ADDRESS"
        exit 1
    else
        echo "$DEPLOY_RESULT"
    fi

    log "... getting deploy hash"
    DEPLOY_HASH=$("$DEPLOY_RESULT"
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
