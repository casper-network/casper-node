#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Submits an auction bid activation - required when ejected from validator set due to liveness fault.
# Arguments:
#   Node ordinal identifier.
#   Flag indicating whether to emit log messages.
#######################################
function main()
{
    local VALIDATOR_ID=${1}
    local QUIET=${2:-"FALSE"}

    local CHAIN_NAME
    local GAS_PAYMENT
    local NODE_ADDRESS
    local PATH_TO_CLIENT
    local PATH_TO_CONTRACT
    local VALIDATOR_ACCOUNT_KEY
    local VALIDATOR_SECRET_KEY

    CHAIN_NAME=$(get_chain_name)
    GAS_PAYMENT=${GAS_PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
    NODE_ADDRESS=$(get_node_address_rpc)
    PATH_TO_CLIENT=$(get_path_to_client)
    PATH_TO_CONTRACT=$(get_path_to_contract "auction/activate_bid.wasm")

    VALIDATOR_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_NODE" "$VALIDATOR_ID")
    VALIDATOR_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_NODE" "$VALIDATOR_ID")

    if [ "$QUIET" != "TRUE" ]; then
        log "dispatching deploy -> activate_bid.wasm"
        log "... chain = $CHAIN_NAME"
        log "... dispatch node = $NODE_ADDRESS"
        log "... contract = $PATH_TO_CONTRACT"
        log "... validator id = $VALIDATOR_ID"
        log "... validator account key = $VALIDATOR_ACCOUNT_KEY"
        log "... validator secret key = $VALIDATOR_SECRET_KEY"
    fi

    DEPLOY_HASH=$(
        $PATH_TO_CLIENT put-deploy \
            --chain-name "$CHAIN_NAME" \
            --node-address "$NODE_ADDRESS" \
            --payment-amount "$GAS_PAYMENT" \
            --ttl "5minutes" \
            --secret-key "$VALIDATOR_SECRET_KEY" \
            --session-arg "$(get_cl_arg_account_key 'validator_public_key' "$VALIDATOR_ACCOUNT_KEY")" \
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

unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        node) NODE_ID=${VALUE} ;;
        validator) NODE_ID=${VALUE} ;;
        *)
    esac
done

main "${NODE_ID:-1}" 
