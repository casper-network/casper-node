#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/contracts-erc20/utils.sh

#######################################
# Approves a token transfer by a user for a specific amount.
# Arguments:
#   Amount of ERC-20 token to permit transfer.
#   User ordinal identifier.
#######################################
function main()
{
    local AMOUNT=${1}
    local USER_ID=${2}
    local CHAIN_NAME
    local GAS_PRICE
    local GAS_PAYMENT
    local NODE_ADDRESS
    local PATH_TO_CLIENT
    local CONTRACT_OWNER_SECRET_KEY
    local USER_ACCOUNT_KEY
    local USER_ACCOUNT_HASH

    # Set standard deploy parameters.
    CHAIN_NAME=$(get_chain_name)
    GAS_PRICE=${GAS_PRICE:-$NCTL_DEFAULT_GAS_PRICE}
    GAS_PAYMENT=${GAS_PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
    NODE_ADDRESS=$(get_node_address_rpc)
    PATH_TO_CLIENT=$(get_path_to_client)

    # Set contract owner secret key.
    CONTRACT_OWNER_SECRET_KEY=$(get_path_to_secret_key "$NCTL_ACCOUNT_TYPE_FAUCET")

    # Set user account key.
    USER_ACCOUNT_KEY=$(get_account_key "$NCTL_ACCOUNT_TYPE_USER" "$USER_ID")

    # Set user account hash.
    USER_ACCOUNT_HASH=$(get_account_hash "$USER_ACCOUNT_KEY")

    # Dispatch deploy (hits node api). 
    DEPLOY_HASH=$(
        $PATH_TO_CLIENT put-deploy \
            --chain-name "$CHAIN_NAME" \
            --gas-price "$GAS_PRICE" \
            --node-address "$NODE_ADDRESS" \
            --payment-amount "$GAS_PAYMENT" \
            --ttl "1day" \
            --secret-key "$CONTRACT_OWNER_SECRET_KEY" \
            --session-name "ERC20" \
            --session-entry-point "approve" \
            --session-arg "$(get_cl_arg_account_hash 'spender' "$USER_ACCOUNT_HASH")" \
            --session-arg "$(get_cl_arg_u256 'amount' "$AMOUNT")" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
    )

    log "ERC20 token transfer approval"
    log "contract details:"
    log "... arg: spender = $USER_ACCOUNT_KEY"
    log "... arg: amount = $AMOUNT"
    log "... entry point = approve"
    log "deploy details:"
    log "... chain = $CHAIN_NAME"
    log "... dispatch node = $NODE_ADDRESS"
    log "... gas payment = $GAS_PAYMENT"
    log "... gas price = $GAS_PRICE"
    log "... hash = $DEPLOY_HASH"
    log "... signing key = $CONTRACT_OWNER_SECRET_KEY"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset AMOUNT
unset USER_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        user) USER_ID=${VALUE} ;;
        *)
    esac
done

main "${AMOUNT:-1000000000}" \
     "${USER_ID:-1}"
