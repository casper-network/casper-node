#!/usr/bin/env bash

unset AMOUNT
unset BATCH_COUNT
unset BATCH_SIZE
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
        count) BATCH_COUNT=${VALUE} ;;
        size) BATCH_SIZE=${VALUE} ;;
        gas) GAS=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        payment) PAYMENT=${VALUE} ;;
        *)
    esac
done

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/transfers/funcs.sh

do_transfer_wasm_prepare \
    ${NET_ID:-1} \
    ${NODE_ID:-1} \
    ${AMOUNT:-$NCTL_DEFAULT_TRANSFER_AMOUNT} \
    ${BATCH_COUNT:-10} \
    ${BATCH_SIZE:-10} \
    $NCTL_DEFAULT_GAS_PRICE \
    $NCTL_DEFAULT_GAS_PAYMENT
