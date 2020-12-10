#!/usr/bin/env bash

#######################################
# Imports
#######################################

source $NCTL/sh/utils.sh

#######################################
# Submits an auction bid.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Validator ordinal identifier.
#   Bid amount.
#   Delegation rate.
#   Gas price.
#   Gas payment.
#######################################
function do_auction_bid_submit()
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local BIDDER_ID=${3}
    local AMOUNT=${4}
    local DELEGATION_RATE=${5}
    local GAS=${6}
    local PAYMENT=${7}

    local BIDDER_PUBLIC_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_NODE $BIDDER_ID)
    local BIDDER_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_NODE $BIDDER_ID)
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local PATH_TO_CONTRACT=$(get_path_to_contract $NET_ID "add_bid.wasm")

    log "dispatching deploy -> add_bid.wasm"
    log "... network = $NET_ID"
    log "... node = $NODE_ID"
    log "... node address = $NODE_ADDRESS"
    log "... contract = $PATH_TO_CONTRACT"
    log "... bidder id = $BIDDER_ID"
    log "... bidder secret key = $BIDDER_SECRET_KEY"
    log "... bid amount = $amount"
    log "... bid delegation rate = $DELEGATION_RATE"

    DEPLOY_HASH=$(
        $(get_path_to_client $NET_ID) put-deploy \
            --chain-name $(get_chain_name $NET_ID) \
            --gas-price $GAS \
            --node-address $NODE_ADDRESS \
            --payment-amount $PAYMENT \
            --secret-key $BIDDER_SECRET_KEY \
            --session-arg="public_key:public_key='$BIDDER_PUBLIC_KEY'" \
            --session-arg "amount:u512='$AMOUNT'" \
            --session-arg "delegation_rate:u64='$DELEGATION_RATE'" \
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
unset DELEGATION_RATE
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
        rate) DELEGATION_RATE=${VALUE} ;;
        *)
    esac
done

do_auction_bid_submit \
    ${NET_ID:-1} \
    ${NODE_ID:-1} \
    ${BIDDER_ID:-1} \
    ${AMOUNT:-$NCTL_DEFAULT_AUCTION_BID_AMOUNT} \
    ${DELEGATION_RATE:-125} \
    ${GAS:-$NCTL_DEFAULT_GAS_PRICE} \
    ${PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
