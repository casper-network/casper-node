#!/usr/bin/env bash

unset ACCOUNT_KEY
unset STATE_ROOT_HASH

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        account-key) ACCOUNT_KEY=${VALUE} ;;
        root-hash) STATE_ROOT_HASH=${VALUE} ;;
        *)
    esac
done

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source "$NCTL"/sh/utils/main.sh

NODE_ADDRESS=$(get_node_address_rpc)
STATE_ROOT_HASH=${STATE_ROOT_HASH:-$(get_state_root_hash)}

$(get_path_to_client) query-global-state \
    --node-address "$NODE_ADDRESS" \
    --state-root-hash "$STATE_ROOT_HASH" \
    --key "$ACCOUNT_KEY" \
    | jq '.result'
