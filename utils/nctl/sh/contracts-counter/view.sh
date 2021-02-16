#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/contracts-counter/utils.sh

#######################################
# Renders ERC-20 token contract details.
#######################################
function main()
{
    local CONTRACT_OWNER_ACCOUNT_KEY
    local CONTRACT_HASH
    local COUNTER_VALUE

    # Set contract owner account key - i.e. faucet account.
    CONTRACT_OWNER_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_FAUCET")

    # Set contract hash (hits node api).
    CONTRACT_HASH=$(get_contract_hash "$CONTRACT_OWNER_ACCOUNT_KEY")

    # Set counter value.
    COUNTER_VALUE=$(get_contract_key_value "$CONTRACT_HASH" "count")

    log "Contract Details -> COUNTER"
    log "... on-chain name = counter"
    log "... on-chain hash = $CONTRACT_HASH"
    log "... owner account = $CONTRACT_OWNER_ACCOUNT_KEY"
    log "... counter value = $COUNTER_VALUE"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

main
