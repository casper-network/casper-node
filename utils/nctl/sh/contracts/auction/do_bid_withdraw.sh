#!/usr/bin/env bash

#######################################
# Imports
#######################################

source $NCTL/sh/utils.sh

#######################################
# Submits an auction withdrawal.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Validator ordinal identifier.
#   Withdrawal amount.
#   Gas price.
#   Gas payment.
#######################################
function do_auction_bid_withdraw() 
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local AMOUNT=${3}
    local BIDDER_ID=${4}
    local GAS=${5}
    local PAYMENT=${6}

    local BIDDER_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_NODE $BIDDER_ID)
    local BIDDER_ACCOUNT_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_NODE $BIDDER_ID)
    local BIDDER_MAIN_PURSE_UREF=$(get_main_purse_uref $NET_ID $NODE_ID $BIDDER_ACCOUNT_KEY)
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local PATH_TO_CONTRACT=$(get_path_to_contract $NET_ID "withdraw_bid.wasm")

    log "dispatching deploy -> withdraw_bid.wasm"
    log "... network = $NET_ID"
    log "... node = $NODE_ID"
    log "... node address = $NODE_ADDRESS"
    log "... contract = $PATH_TO_CONTRACT"
    log "... bidder id = $BIDDER_ID"
    log "... bidder account key = $BIDDER_ACCOUNT_KEY"
    log "... bidder secret key = $BIDDER_SECRET_KEY"
    log "... bidder main purse uref = $BIDDER_MAIN_PURSE_UREF"
    log "... withdrawal amount = $AMOUNT"

    DEPLOY_HASH=$(
        $(get_path_to_client $NET_ID) put-deploy \
            --chain-name $(get_chain_name $NET_ID) \
            --gas-price $GAS \
            --node-address $NODE_ADDRESS \
            --payment-amount $PAYMENT \
            --secret-key $BIDDER_SECRET_KEY \
            --session-arg="public_key:public_key='$BIDDER_ACCOUNT_KEY'" \
            --session-arg "amount:u512='$AMOUNT'" \
            --session-arg "unbond_purse:opt_uref='$BIDDER_MAIN_PURSE_UREF'" \
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
unset BIDDER_ID
unset GAS
unset NET_ID
unset NODE_ID
unset PAYMENT

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        bidder) BIDDER_ID=${VALUE} ;;
        gas) GAS=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        payment) PAYMENT=${VALUE} ;;
        *)
    esac
done

do_auction_bid_withdraw \
    ${NET_ID:-1} \
    ${NODE_ID:-1} \
    ${BIDDER_ID:-1} \
    ${AMOUNT:-$NCTL_DEFAULT_AUCTION_BID_AMOUNT} \
    ${GAS:-$NCTL_DEFAULT_GAS_PRICE} \
    ${PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
