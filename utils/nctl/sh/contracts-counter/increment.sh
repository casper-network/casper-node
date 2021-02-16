#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/contracts-counter/utils.sh

#######################################
# KV-STORAGE: sets value of key stored under contract.
# Arguments:
#   Name of key to be written.
#   Type of key to be written.
#   Value of key to be written.
#######################################
function main()
{
    local CHAIN_NAME
    local GAS_PRICE
    local GAS_PAYMENT
    local NODE_ADDRESS
    local PATH_TO_CLIENT
    local CONTRACT_HASH
    local CONTRACT_OWNER_ACCOUNT_KEY
    local CONTRACT_OWNER_SECRET_KEY

    # Set standard deploy parameters.
    CHAIN_NAME=$(get_chain_name)
    GAS_PRICE=${GAS_PRICE:-$NCTL_DEFAULT_GAS_PRICE}
    GAS_PAYMENT=${GAS_PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
    NODE_ADDRESS=$(get_node_address_rpc)
    PATH_TO_CLIENT=$(get_path_to_client)

    # Set contract owner account key - i.e. faucet account.
    CONTRACT_OWNER_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_FAUCET")

    # Set contract owner secret key.
    CONTRACT_OWNER_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_FAUCET")

    # Set contract hash (hits node api).
    CONTRACT_HASH=$(get_contract_hash "$CONTRACT_OWNER_ACCOUNT_KEY")
    echo $CONTRACT_HASH

    # Set contract path.
    PATH_TO_CONTRACT=$(get_path_to_contract "counter_call.wasm")
    if [ ! -f "$PATH_TO_CONTRACT" ]; then
        echo "ERROR: The counter_call.wasm binary file cannot be found.  Please compile it and move it to the following directory: $(get_path_to_net)"
        return
    fi

    # Dispatch deploy (hits node api). 
    DEPLOY_HASH=$(
        $PATH_TO_CLIENT put-deploy \
            --chain-name "$CHAIN_NAME" \
            --gas-price "$GAS_PRICE" \
            --node-address "$NODE_ADDRESS" \
            --payment-amount "$GAS_PAYMENT" \
            --secret-key "$CONTRACT_OWNER_SECRET_KEY" \
            --ttl "1day" \
            --session-hash "$CONTRACT_HASH" \
            --session-entry-point "counter_inc" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )
    log "increment counter deploy hash = $DEPLOY_HASH"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

main
