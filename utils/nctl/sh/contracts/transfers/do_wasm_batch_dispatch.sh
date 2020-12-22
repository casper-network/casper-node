#!/usr/bin/env bash

unset NET_ID
unset NODE_ID
unset TRANSFER_INTERVAL

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        interval) TRANSFER_INTERVAL=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/transfers/funcs.sh

do_transfer_wasm_dispatch \
    ${NET_ID:-1} \
    ${NODE_ID:-1} \
    ${TRANSFER_INTERVAL:-0.01} \
