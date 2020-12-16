#!/usr/bin/env bash

source $NCTL/sh/utils.sh

unset NET_ID
unset NODE_ID
unset PURSE_UREF
unset STATE_ROOT_HASH
unset PREFIX

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        purse-uref) PURSE_UREF=${VALUE} ;;
        root-hash) STATE_ROOT_HASH=${VALUE} ;;
        prefix) PREFIX=${VALUE} ;;
        *)
    esac
done

NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-1}
PREFIX=${PREFIX:-"account"}
STATE_ROOT_HASH=${STATE_ROOT_HASH:-$(get_state_root_hash $NET_ID $NODE_ID)}

ACCOUNT_BALANCE=$(
    $(get_path_to_client $NET_ID) get-balance \
        --node-address $(get_node_address_rpc $NET_ID $NODE_ID) \
        --state-root-hash $STATE_ROOT_HASH \
        --purse-uref $PURSE_UREF \
        | jq '.result.balance_value' \
        | sed -e 's/^"//' -e 's/"$//'
    )

log $prefix" balance = "$ACCOUNT_BALANCE
