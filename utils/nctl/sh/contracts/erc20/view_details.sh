#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/erc20/utils.sh

#######################################
# Renders ERC-20 token contract details.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function main()
{
    # Set contract owner account key - i.e. faucet account.
    local CONTRACT_OWNER_ACCOUNT_KEY=$(get_account_key $NCTL_ACCOUNT_TYPE_FAUCET)

    # Set contract hash (hits node api).
    local CONTRACT_HASH=$(get_erc20_contract_hash $CONTRACT_OWNER_ACCOUNT_KEY)

    # Set token name (hits node api).
    local TOKEN_NAME=$(get_erc20_contract_key_value $CONTRACT_HASH "_name")

    # Set token symbol (hits node api).
    local TOKEN_SYMBOL=$(get_erc20_contract_key_value $CONTRACT_HASH "_symbol")

    # Set token supply (hits node api).
    local TOKEN_SUPPLY=$(get_erc20_contract_key_value $CONTRACT_HASH "_totalSupply")

    # Set token decimals (hits node api).
    local TOKEN_DECIMALS=$(get_erc20_contract_key_value $CONTRACT_HASH "_decimals")

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
