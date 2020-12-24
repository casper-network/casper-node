#!/usr/bin/env bash

source $NCTL/sh/utils/main.sh
source $NCTL/sh/contracts/erc20/utils.sh

#######################################
# Renders ERC-20 token contract balances.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function main()
{
    # Set contract owner account key - i.e. faucet account.
    local CONTRACT_OWNER_ACCOUNT_KEY=$(get_account_key $NCTL_ACCOUNT_TYPE_FAUCET)

    # Set contract owner account hash.
    local CONTRACT_OWNER_ACCOUNT_HASH=$(get_account_hash $CONTRACT_OWNER_ACCOUNT_KEY)

    # Set contract owner ERC-20 balance key.
    local CONTRACT_OWNER_ACCOUNT_BALANCE_KEY="_balances_$CONTRACT_OWNER_ACCOUNT_HASH"

    # Set contract hash (hits node api).
    local CONTRACT_HASH=$(get_erc20_contract_hash $CONTRACT_OWNER_ACCOUNT_KEY)

    # Set contract owner account balance (hits node api).
    local CONTRACT_OWNER_ACCOUNT_BALANCE=$(get_erc20_contract_key_value $CONTRACT_HASH $CONTRACT_OWNER_ACCOUNT_BALANCE_KEY)

    # Set token symbol (hits node api).
    local TOKEN_SYMBOL=$(get_erc20_contract_key_value $CONTRACT_HASH "_symbol")

    log "ERC-20 $TOKEN_SYMBOL Account Balances:"
    log "... contract owner = "$CONTRACT_OWNER_ACCOUNT_BALANCE

    # Render user account balances.
    for USER_ID in $(seq 1 $(get_count_of_users))
    do
        # Set user account key.
        local USER_ACCOUNT_KEY=$(get_account_key $net $NCTL_ACCOUNT_TYPE_USER $USER_ID)

        # Set user account hash.
        local USER_ACCOUNT_HASH=$(get_account_hash $USER_ACCOUNT_KEY)

        # Set user ERC-20 balance key.
        local USER_ACCOUNT_BALANCE_KEY="_balances_$USER_ACCOUNT_HASH"

        # Set user account balance (hits node api).
        local USER_ACCOUNT_BALANCE=$(get_erc20_contract_key_value $CONTRACT_HASH $USER_ACCOUNT_BALANCE_KEY)

        log "... user $USER_ID = $USER_ACCOUNT_BALANCE"
    done
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

main
