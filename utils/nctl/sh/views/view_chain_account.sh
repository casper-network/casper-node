#!/usr/bin/env bash

unset ACCOUNT_KEY
unset NET_ID
unset NODE_ID
unset STATE_ROOT_HASH

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        account-key) ACCOUNT_KEY=${VALUE} ;;
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        root-hash) STATE_ROOT_HASH=${VALUE} ;;
        *)
    esac
done

NET_ID=${NET_ID:-1}
NODE_ID=${NODE_ID:-1}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/utils.sh

STATE_ROOT_HASH=${STATE_ROOT_HASH:-$(get_state_root_hash $NET_ID $NODE_ID)}

$(get_path_to_client $NET_ID) query-state \
    --node-address $(get_node_address_rpc $NET_ID $NODE_ID) \
    --state-root-hash $STATE_ROOT_HASH \
    --key $ACCOUNT_KEY \
    | jq '.result'
