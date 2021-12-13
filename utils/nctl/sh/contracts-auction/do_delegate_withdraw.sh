#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Submits an auction delegate withdrawal.
# Arguments:
#   Amount to delegate.
#   Delegator ordinal identifier.
#   Validator ordinal identifier.
#######################################
function main()
{
    local AMOUNT=${1}
    local DELEGATOR_ID=${2}
    local VALIDATOR_ID=${3}
    local CHAIN_NAME
    local GAS_PRICE
    local GAS_PAYMENT
    local NODE_ADDRESS
    local PATH_TO_CLIENT
    local PATH_TO_CONTRACT
    local DELEGATOR_ACCOUNT_KEY
    local DELEGATOR_SECRET_KEY
    local DELEGATOR_MAIN_PURSE_UREF
    local VALIDATOR_ACCOUNT_KEY

    CHAIN_NAME=$(get_chain_name)
    GAS_PRICE=${GAS_PRICE:-$NCTL_DEFAULT_GAS_PRICE}
    GAS_PAYMENT=${GAS_PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
    NODE_ADDRESS=$(get_node_address_rpc)
    PATH_TO_CLIENT=$(get_path_to_client)
    PATH_TO_CONTRACT=$(get_path_to_contract "auction/undelegate.wasm")

    DELEGATOR_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_USER" "$DELEGATOR_ID" | tr '[:upper:]' '[:lower:]')
    DELEGATOR_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_USER" "$DELEGATOR_ID")
    DELEGATOR_MAIN_PURSE_UREF=$(get_main_purse_uref "$DELEGATOR_ACCOUNT_KEY" | tr '[:upper:]' '[:lower:]')
    VALIDATOR_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_NODE" "$VALIDATOR_ID" | tr '[:upper:]' '[:lower:]')

    log "dispatching deploy -> undelegate.wasm"
    log "... chain = $CHAIN_NAME"
    log "... dispatch node = $NODE_ADDRESS"
    log "... contract = $PATH_TO_CONTRACT"
    log "... delegator id = $DELEGATOR_ID"
    log "... delegator account key = $DELEGATOR_ACCOUNT_KEY"
    log "... delegator secret key = $DELEGATOR_SECRET_KEY"
    log "... delegator main purse uref = $DELEGATOR_MAIN_PURSE_UREF"
    log "... amount = $AMOUNT"

    DEPLOY_HASH=$(
        $PATH_TO_CLIENT put-deploy \
            --chain-name "$CHAIN_NAME" \
            --gas-price "$GAS_PRICE" \
            --node-address "$NODE_ADDRESS" \
            --payment-amount "$GAS_PAYMENT" \
            --ttl "1day" \
            --secret-key "$DELEGATOR_SECRET_KEY" \
            --session-arg "$(get_cl_arg_u512 'amount' "$AMOUNT")" \
            --session-arg "$(get_cl_arg_account_key 'delegator' "$DELEGATOR_ACCOUNT_KEY")" \
            --session-arg "$(get_cl_arg_account_key 'validator' "$VALIDATOR_ACCOUNT_KEY")" \
            --session-arg "$(get_cl_arg_opt_uref 'unbond_purse' "$DELEGATOR_MAIN_PURSE_UREF")" \
            --session-path "$PATH_TO_CONTRACT" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )

    log "deploy dispatched:"
    log "... deploy hash = $DEPLOY_HASH"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset AMOUNT
unset DELEGATOR_ID
unset VALIDATOR_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        delegator) DELEGATOR_ID=${VALUE} ;;
        validator) VALIDATOR_ID=${VALUE} ;;
        *)
    esac
done

main \
    "${AMOUNT:-$NCTL_DEFAULT_AUCTION_DELEGATE_AMOUNT}" \
    "${DELEGATOR_ID:-1}" \
    "${VALIDATOR_ID:-1}"
