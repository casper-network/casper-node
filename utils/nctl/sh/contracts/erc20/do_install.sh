#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/erc20/funcs.sh

unset GAS_PAYMENT
unset GAS_PRICE
unset NET_ID
unset NODE_ID
unset TOKEN_NAME
unset TOKEN_SUPPLY
unset TOKEN_SYMBOL

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        gas) GAS_PRICE=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        name) TOKEN_NAME=${VALUE} ;;
        payment) GAS_PAYMENT=${VALUE} ;;
        supply) TOKEN_SUPPLY=${VALUE} ;;
        symbol) TOKEN_SYMBOL=${VALUE} ;;
        *)
    esac
done

do_erc20_install_contract \
    ${NET_ID:-1} \
    ${NODE_ID:-1} \
    ${TOKEN_NAME:-"Acme Token"} \
    ${TOKEN_SYMBOL:-"ACME"} \
    ${TOKEN_SUPPLY:-1000000000000000000000000000000000} \
    ${GAS_PRICE:-$NCTL_DEFAULT_GAS_PRICE} \
    ${GAS_PAYMENT:-70000000000}
