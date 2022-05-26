#!/usr/bin/env bash

unset PURSE_UREF
unset STATE_ROOT_HASH
unset PREFIX

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        purse-uref) PURSE_UREF=${VALUE} ;;
        root-hash) STATE_ROOT_HASH=${VALUE} ;;
        prefix) PREFIX=${VALUE} ;;
        *)
    esac
done

PREFIX=${PREFIX:-"account"}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source "$NCTL"/sh/utils/main.sh

NODE_ADDRESS=$(get_node_address_rpc)
STATE_ROOT_HASH=${STATE_ROOT_HASH:-$(get_state_root_hash)}

ACCOUNT_BALANCE=$(
    $(get_path_to_client) query-balance \
        --node-address "$NODE_ADDRESS" \
        --state-root-hash "$STATE_ROOT_HASH" \
        --purse-uref "$PURSE_UREF" \
        | jq '.result.balance' \
        | sed -e 's/^"//' -e 's/"$//'
    )

log "$PREFIX balance = $ACCOUNT_BALANCE"
