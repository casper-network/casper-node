#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/erc20/funcs.sh

unset NET_ID
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

render_erc20_contract_balances \
    $NET_ID \
    $NODE_ID
