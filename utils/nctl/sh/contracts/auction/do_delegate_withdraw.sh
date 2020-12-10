#!/usr/bin/env bash

#######################################
# Imports
#######################################

source $NCTL/sh/utils.sh

#######################################
# Submits an auction delegate withdrawal.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Delegator ordinal identifier.
#   Validator ordinal identifier.
#   Amount to delegate.
#   Gas price.
#   Gas payment.
#######################################
function do_auction_delegate_withdraw()
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local DELEGATOR_ID=${3}
    local VALIDATOR_ID=${4}
    local AMOUNT=${5}
    local GAS=${6}
    local PAYMENT=${7}

    local DELEGATOR_ACCOUNT_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $DELEGATOR_ID)
    local DELEGATOR_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $DELEGATOR_ID)
    local DELEGATOR_MAIN_PURSE_UREF=$(get_main_purse_uref $NET_ID $NODE_ID $DELEGATOR_ACCOUNT_KEY)
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local PATH_TO_CONTRACT=$(get_path_to_contract $NET_ID "undelegate.wasm")
    local VALIDATOR_PUBLIC_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_NODE $VALIDATOR_ID)

    log "dispatching deploy -> undelegate.wasm"
    log "... network = $NET_ID"
    log "... node = $NODE_ID"
    log "... node address = $NODE_ADDRESS"
    log "... contract = $PATH_TO_CONTRACT"
    log "... delegator id = $DELEGATOR_ID"
    log "... delegator account key = $DELEGATOR_ACCOUNT_KEY"
    log "... delegator secret key = $DELEGATOR_SECRET_KEY"
    log "... delegator main purse uref = $DELEGATOR_MAIN_PURSE_UREF"
    log "... amount = $AMOUNT"

    DEPLOY_HASH=$(
        $(get_path_to_client $NET_ID) put-deploy \
            --chain-name $(get_chain_name $NET_ID) \
            --gas-price $GAS \
            --node-address $NODE_ADDRESS \
            --payment-amount $PAYMENT \
            --secret-key $DELEGATOR_SECRET_KEY \
            --session-arg "amount:u512='$AMOUNT'" \
            --session-arg "delegator:public_key='$DELEGATOR_ACCOUNT_KEY'" \
            --session-arg "validator:public_key='$VALIDATOR_PUBLIC_KEY'" \
            --session-arg "unbond_purse:opt_uref='$DELEGATOR_MAIN_PURSE_UREF'" \
            --session-path $PATH_TO_CONTRACT \
            --ttl "1day" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )

    log "deploy dispatched:"
    log "... deploy hash = $DEPLOY_HASH"
}

#######################################
# Destructure input args.
#######################################

unset AMOUNT
unset GAS
unset NET_ID
unset NODE_ID
unset PAYMENT
unset DELEGATOR_ID
unset VALIDATOR_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        gas) GAS=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        payment) PAYMENT=${VALUE} ;;
        delegator) DELEGATOR_ID=${VALUE} ;;
        validator) VALIDATOR_ID=${VALUE} ;;
        *)
    esac
done

do_auction_delegate_withdraw \
    ${NET_ID:-1} \
    ${NODE_ID:-1} \
    ${DELEGATOR_ID:-1} \
    ${VALIDATOR_ID:-1} \
    ${AMOUNT:-$NCTL_DEFAULT_AUCTION_DELEGATE_AMOUNT} \
    ${GAS:-$NCTL_DEFAULT_GAS_PRICE} \
    ${PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
