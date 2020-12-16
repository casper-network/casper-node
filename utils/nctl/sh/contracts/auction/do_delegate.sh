#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/auction/funcs.sh

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

do_auction_delegate_submit \
    ${NET_ID:-1} \
    ${NODE_ID:-1} \
    ${DELEGATOR_ID:-1} \
    ${VALIDATOR_ID:-1} \
    ${AMOUNT:-$NCTL_DEFAULT_AUCTION_DELEGATE_AMOUNT} \
    ${GAS:-$NCTL_DEFAULT_GAS_PRICE} \
    ${PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
