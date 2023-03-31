#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/contracts-kv/utils.sh

#######################################
# KV-STORAGE: sets value of key stored under contract.
# Arguments:
#   Name of key to be written.
#   Type of key to be written.
#   Value of key to be written.
#######################################
function main()
{
    local KEY_NAME=${1}
    local KEY_TYPE=${2}
    local KEY_VALUE=${3}
    local CHAIN_NAME
    local GAS_PAYMENT
    local NODE_ADDRESS
    local PATH_TO_CLIENT
    local CONTRACT_HASH
    local CONTRACT_OWNER_ACCOUNT_KEY
    local CONTRACT_OWNER_SECRET_KEY

    # Set standard deploy parameters.
    CHAIN_NAME=$(get_chain_name)
    GAS_PAYMENT=${GAS_PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
    NODE_ADDRESS=$(get_node_address_rpc)
    PATH_TO_CLIENT=$(get_path_to_client)

    # Set contract owner account key - i.e. faucet account.
    CONTRACT_OWNER_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_FAUCET")

    # Set contract owner secret key.
    CONTRACT_OWNER_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_FAUCET")

    # Set contract hash (hits node api).
    CONTRACT_HASH=$(get_kv_contract_hash "$CONTRACT_OWNER_ACCOUNT_KEY")

    # Dispatch deploy (hits node api). 
    DEPLOY_HASH=$(
        $PATH_TO_CLIENT put-deploy \
            --chain-name "$CHAIN_NAME" \
            --node-address "$NODE_ADDRESS" \
            --payment-amount "$GAS_PAYMENT" \
            --secret-key "$CONTRACT_OWNER_SECRET_KEY" \
            --ttl "5minutes" \
            --session-hash "$CONTRACT_HASH" \
            --session-entry-point "$(get_contract_entry_point "$KEY_TYPE")" \
            --session-arg "$(get_cl_arg_string 'name' "$KEY_NAME")" \
            --session-arg "$(get_key_value_session_arg "$KEY_TYPE" "$KEY_VALUE")" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )
    log "set key deploy hash = $DEPLOY_HASH"
}

#######################################
# Returns contract entry point mapped from key type.
#######################################
function get_contract_entry_point () 
{
    local KEY_TYPE=${1}

    if [ "$KEY_TYPE" == "string" ]; then
        echo "store_string"
    elif [ "$KEY_TYPE" == "u64" ]; then
        echo "store_u64"
    elif [ "$KEY_TYPE" == "u512" ]; then
        echo "store_u512"
    elif [ "$KEY_TYPE" == "account-hash" ]; then
        echo "store_account_hash"
    else
        echo "store_string"
    fi
}

#######################################
# Returns contract key value session arg mapped from key type.
#######################################
function get_key_value_session_arg () 
{
    local KEY_TYPE=${1}
    local KEY_VALUE=${2}

    if [ "$KEY_TYPE" == "string" ]; then
        get_cl_arg_string 'value' "$KEY_VALUE"
    elif [ "$KEY_TYPE" == "u64" ]; then
        get_cl_arg_u64 'value' "$KEY_VALUE"
    elif [ "$KEY_TYPE" == "u512" ]; then
        get_cl_arg_u512 'value' "$KEY_VALUE"
    elif [ "$KEY_TYPE" == "account-hash" ]; then
        get_cl_arg_account_hash 'value' "$KEY_VALUE"
    else
        get_cl_arg_string 'value' "$KEY_VALUE"
    fi
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset KEY_NAME
unset KEY_TYPE
unset KEY_VALUE

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        key) KEY_NAME=${VALUE} ;;
        type) KEY_TYPE=${VALUE} ;;
        value) KEY_VALUE=${VALUE} ;;
        *)
    esac
done

main \
    "${KEY_NAME:-"main"}" \
    "${KEY_TYPE:-"string"}" \
    "${KEY_VALUE:-"hello dolly"}"
