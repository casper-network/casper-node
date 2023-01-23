#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/contracts-kv/utils.sh

#######################################
# KV-STORAGE: Installs contract under network faucet account.
#######################################
function main()
{
    local CHAIN_NAME
    local GAS_PAYMENT
    local NODE_ADDRESS
    local PATH_TO_CLIENT
    local PATH_TO_CONTRACT
    local CONTRACT_OWNER_SECRET_KEY
    local CONTRACT_ARG_MESSAGE="Hello Dolly"

    # Set standard deploy parameters.
    CHAIN_NAME=$(get_chain_name)
    GAS_PAYMENT=10000000000000
    NODE_ADDRESS=$(get_node_address_rpc)
    PATH_TO_CLIENT=$(get_path_to_client)

    # Set contract path.
    PATH_TO_CONTRACT=$(get_path_to_contract "eco/hello_world.wasm")
    if [ ! -f "$PATH_TO_CONTRACT" ]; then
        echo "ERROR: The hello_world.wasm binary file cannot be found.  Please compile it and move it to the following directory: $(get_path_to_net)"
        return
    fi

    # Set contract owner secret key.
    CONTRACT_OWNER_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_FAUCET")

    log "installing contract -> KV storage"
    log "... chain = $CHAIN_NAME"
    log "... dispatch node = $NODE_ADDRESS"
    log "... gas payment = $GAS_PAYMENT"
    log "contract constructor args:"
    log "... message = $CONTRACT_ARG_MESSAGE"
    log "contract installation details:"
    log "... path = $PATH_TO_CONTRACT"

    # Dispatch deploy (hits node api).
    DEPLOY_HASH=$(
        $PATH_TO_CLIENT put-deploy \
            --chain-name "$CHAIN_NAME" \
            --node-address "$NODE_ADDRESS" \
            --payment-amount "$GAS_PAYMENT" \
            --ttl "5min" \
            --secret-key "$CONTRACT_OWNER_SECRET_KEY" \
            --session-path "$PATH_TO_CONTRACT" \
            --session-arg "$(get_cl_arg_string 'message' "$CONTRACT_ARG_MESSAGE")" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )

    log "... deploy hash = $DEPLOY_HASH"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

main 
