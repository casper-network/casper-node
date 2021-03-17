#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/contracts-kv/utils.sh

#######################################
# KV-STORAGE: gets key stored within contract storage.
# Arguments:
#   Name of key to be read.
#######################################
function main()
{
    local KEY_NAME=${1}
    local CONTRACT_OWNER_ACCOUNT_KEY
    local CONTRACT_HASH

    # Set contract owner account key - i.e. faucet account.
    CONTRACT_OWNER_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_FAUCET")

    # Set contract hash (hits node api).
    CONTRACT_HASH=$(get_kv_contract_hash "$CONTRACT_OWNER_ACCOUNT_KEY")

    # Set stored key value (hits node api).
    KEY_VALUE=$(get_kv_contract_key_value "$CONTRACT_HASH" "$KEY_NAME")

    log "Contract Details -> KV-STORAGE"
    log "... contract name     : kvstorage_contract"
    log "... contract hash     : $CONTRACT_HASH"
    log "... contract owner    : $CONTRACT_OWNER_ACCOUNT_KEY"
    log "... key name          : $KEY_NAME"
    log "... key value type    : $(jq -n --argjson data "$KEY_VALUE" '$data.cl_type')"
    log "... key value raw     : $(jq -n --argjson data "$KEY_VALUE" '$data.bytes')"
    log "... key value parsed  : $(jq -n --argjson data "$KEY_VALUE" '$data.parsed')"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset KEY_NAME

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        key) KEY_NAME=${VALUE} ;;
        *)
    esac
done

main \
    "${KEY_NAME:-"main"}" 
