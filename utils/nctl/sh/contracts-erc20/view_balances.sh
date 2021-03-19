#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/contracts-erc20/utils.sh

#######################################
# Renders ERC-20 token contract balances.
#######################################
function main()
{
    local CONTRACT_OWNER_ACCOUNT_KEY
    local CONTRACT_OWNER_ACCOUNT_HASH
    local CONTRACT_OWNER_ACCOUNT_BALANCE_KEY
    local CONTRACT_HASH
    local CONTRACT_OWNER_ACCOUNT_BALANCE
    local TOKEN_SYMBOL
    local USER_ID
    local USER_ACCOUNT_KEY
    local USER_ACCOUNT_HASH
    local USER_ACCOUNT_BALANCE_KEY
    local USER_ACCOUNT_BALANCE

    # Set contract owner account key - i.e. faucet account.
    CONTRACT_OWNER_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_FAUCET")

    # Set contract owner account hash.
    CONTRACT_OWNER_ACCOUNT_HASH=$(get_account_hash "$CONTRACT_OWNER_ACCOUNT_KEY")

    # Set contract owner ERC-20 balance key.
    CONTRACT_OWNER_ACCOUNT_BALANCE_KEY="_balances_$CONTRACT_OWNER_ACCOUNT_HASH"

    # Set contract hash (hits node api).
    CONTRACT_HASH=$(get_erc20_contract_hash "$CONTRACT_OWNER_ACCOUNT_KEY")
    log "ERC-20 $TOKEN_SYMBOL contract:"
    log "... contract hash = $CONTRACT_HASH"

    # Set contract owner account balance (hits node api).
    CONTRACT_OWNER_ACCOUNT_BALANCE=$(get_erc20_contract_key_value "$CONTRACT_HASH" "$CONTRACT_OWNER_ACCOUNT_BALANCE_KEY")

    # Set token symbol (hits node api).
    TOKEN_SYMBOL=$(get_erc20_contract_key_value "$CONTRACT_HASH" "_symbol")

    log "... account balances:"
    log "... ... contract owner = $CONTRACT_OWNER_ACCOUNT_BALANCE"

    # Render user account balances.
    for USER_ID in $(seq 1 "$(get_count_of_users)")
    do
        # Set user account key.
        USER_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_USER" "$USER_ID")

        # Set user account hash.
        USER_ACCOUNT_HASH=$(get_account_hash "$USER_ACCOUNT_KEY")

        # Set user ERC-20 balance key.
        USER_ACCOUNT_BALANCE_KEY="_balances_$USER_ACCOUNT_HASH"

        # Set user account balance (hits node api).
        USER_ACCOUNT_BALANCE=$(get_erc20_contract_key_value "$CONTRACT_HASH" "$USER_ACCOUNT_BALANCE_KEY")

        log "... ... user $USER_ID = $USER_ACCOUNT_BALANCE"
    done
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

main
