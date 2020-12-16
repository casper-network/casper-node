#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/auction/funcs.sh

unset AMOUNT
unset BIDDER_ID
unset GAS
unset NET_ID
unset NODE_ID
unset PAYMENT
unset QUIET

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
        quiet) QUIET=${VALUE} ;;
        *)
    esac
done

do_auction_bid_withdraw \
    ${NET_ID:-1} \
    ${NODE_ID:-1} \
    ${BIDDER_ID:-1} \
    ${AMOUNT:-$NCTL_DEFAULT_AUCTION_BID_AMOUNT} \
    ${GAS:-$NCTL_DEFAULT_GAS_PRICE} \
    ${PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT} \
    ${QUIET:-"FALSE"}
