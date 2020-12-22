#!/usr/bin/env bash

unset AMOUNT
unset GAS
unset TRANSFER_INTERVAL
unset NET_ID
unset NODE_ID
unset PAYMENT
unset TRANSFERS
unset USER_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        gas) GAS=${VALUE} ;;
        interval) TRANSFER_INTERVAL=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        payment) PAYMENT=${VALUE} ;;
        transfers) TRANSFERS=${VALUE} ;;
        user) USER_ID=${VALUE} ;;
        *)
    esac
done

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/transfers/funcs.sh

do_transfer_native \
    ${NET_ID:-1} \
    ${NODE_ID:-1} \
    ${AMOUNT:-$NCTL_DEFAULT_TRANSFER_AMOUNT} \
    ${USER_ID:-1} \
    ${TRANSFERS:-100} \
    ${TRANSFER_INTERVAL:-0.01} \
    ${GAS:-$NCTL_DEFAULT_GAS_PRICE} \
    ${PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
