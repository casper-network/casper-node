#!/usr/bin/env bash

#######################################
# KV-STORAGE: get on-chain contract hash.
# Arguments:
#   Contract owner account key.
#######################################
function get_kv_contract_hash ()
{
    local ACCOUNT_KEY=${1}

    $(get_path_to_client) query-global-state \
        --node-address "$(get_node_address_rpc)" \
        --state-root-hash "$(get_state_root_hash)" \
        --key "$ACCOUNT_KEY" \
        | jq '.result.stored_value.Account.named_keys[] | select(.name == "kvstorage_contract") | .key' \
        | sed -e 's/^"//' -e 's/"$//'
}

#######################################
# KV-STORAGE: get on-chain contract key value.
# Arguments:
#   Contract owner account key.
#   State query path.
#######################################
function get_kv_contract_key_value ()
{
    local QUERY_KEY=${1}
    local QUERY_PATH=${2}

    $(get_path_to_client) query-global-state \
        --node-address "$(get_node_address_rpc)" \
        --state-root-hash "$(get_state_root_hash)" \
        --key "$QUERY_KEY" \
        --query-path "$QUERY_PATH" \
        | jq '.result.stored_value.CLValue';
}
