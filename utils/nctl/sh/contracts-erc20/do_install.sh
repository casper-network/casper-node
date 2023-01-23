#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/contracts-erc20/utils.sh

#######################################
# Installs ERC-20 token contract under network faucet account.
# Arguments:
#   Name of ERC-20 token being created.
#   Symbol associated with ERC-20 token.
#   Total supply of ERC-20 token.
#######################################
function main()
{
    local TOKEN_NAME=${1}
    local TOKEN_SYMBOL=${2}
    local TOKEN_SUPPLY=${3}
    local CHAIN_NAME
    local GAS_PAYMENT
    local NODE_ADDRESS
    local PATH_TO_CLIENT
    local PATH_TO_CONTRACT
    local CONTRACT_OWNER_SECRET_KEY

    # Set standard deploy parameters.
    CHAIN_NAME=$(get_chain_name)
    GAS_PAYMENT=10000000000000
    NODE_ADDRESS=$(get_node_address_rpc)
    PATH_TO_CLIENT=$(get_path_to_client)

    # Set contract path.
    PATH_TO_CONTRACT=$(get_path_to_contract "eco/erc20.wasm")
    if [ ! -f "$PATH_TO_CONTRACT" ]; then
        echo "ERROR: The erc20.wasm binary file cannot be found.  Please compile it and move it to the following directory: $(get_path_to_net)"
        return
    fi

    # Set contract owner secret key.
    CONTRACT_OWNER_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_FAUCET")

    log "installing contract -> ERC-20"
    log "... chain = $CHAIN_NAME"
    log "... dispatch node = $NODE_ADDRESS"
    log "... gas payment = $GAS_PAYMENT"
    log "contract constructor args:"
    log "... token name = $TOKEN_NAME"
    log "... token symbol = $TOKEN_SYMBOL"
    log "... token supply = $TOKEN_SUPPLY"
    log "contract installation details:"
    log "... path = $PATH_TO_CONTRACT"

    # Dispatch deploy (hits node api).
    DEPLOY_HASH=$(
        $PATH_TO_CLIENT put-deploy \
            --chain-name "$CHAIN_NAME" \
            --node-address "$NODE_ADDRESS" \
            --payment-amount "$GAS_PAYMENT" \
            --ttl "5minutes" \
            --secret-key "$CONTRACT_OWNER_SECRET_KEY" \
            --session-path "$PATH_TO_CONTRACT" \
            --session-arg "$(get_cl_arg_string 'tokenName' "$TOKEN_NAME")" \
            --session-arg "$(get_cl_arg_string 'tokenSymbol' "$TOKEN_SYMBOL")" \
            --session-arg "$(get_cl_arg_u256 'tokenTotalSupply' "$TOKEN_SUPPLY")" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )

    log "... deploy hash = $DEPLOY_HASH"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset TOKEN_NAME
unset TOKEN_SUPPLY
unset TOKEN_SYMBOL

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        name) TOKEN_NAME=${VALUE} ;;
        supply) TOKEN_SUPPLY=${VALUE} ;;
        symbol) TOKEN_SYMBOL=${VALUE} ;;
        *)
    esac
done

main "${TOKEN_NAME:-"Acme Token"}" \
     "${TOKEN_SYMBOL:-"ACME"}" \
     "${TOKEN_SUPPLY:-1000000000000000000000000000000000}"
