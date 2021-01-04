#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/contracts-erc20/utils.sh

#######################################
# Renders ERC-20 token contract details.
#######################################
function main()
{
    local CONTRACT_OWNER_ACCOUNT_KEY
    local CONTRACT_HASH
    local TOKEN_NAME
    local TOKEN_SYMBOL
    local TOKEN_SUPPLY
    local TOKEN_DECIMALS

    # Set contract owner account key - i.e. faucet account.
    CONTRACT_OWNER_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_FAUCET")

    # Set contract hash (hits node api).
    CONTRACT_HASH=$(get_erc20_contract_hash "$CONTRACT_OWNER_ACCOUNT_KEY")

    # Set token name (hits node api).
    TOKEN_NAME=$(get_erc20_contract_key_value "$CONTRACT_HASH" "_name")

    # Set token symbol (hits node api).
    TOKEN_SYMBOL=$(get_erc20_contract_key_value "$CONTRACT_HASH" "_symbol")

    # Set token supply (hits node api).
    TOKEN_SUPPLY=$(get_erc20_contract_key_value "$CONTRACT_HASH" "_totalSupply")

    # Set token decimals (hits node api).
    TOKEN_DECIMALS=$(get_erc20_contract_key_value "$CONTRACT_HASH" "_decimals")

    log "Contract Details -> ERC-20"
    log "... on-chain name = ERC20"
    log "... on-chain hash = $CONTRACT_HASH"
    log "... owner account = $CONTRACT_OWNER_ACCOUNT_KEY"
    log "... token name = $TOKEN_NAME"
    log "... token symbol = $TOKEN_SYMBOL"
    log "... token supply = $TOKEN_SUPPLY"
    log "... token decimals = $TOKEN_DECIMALS"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

main
