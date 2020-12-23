#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $NCTL/sh/contracts/erc20/utils.sh

#######################################
# Transfers ERC-20 tokens from one user to another - i.e. peer to peer transfer.
# Arguments:
#   Amount of ERC-20 token to transfer.
#   User 1 ordinal identifier.
#   User 2 ordinal identifier.
#######################################
function main()
{
    local AMOUNT=${1}
    local USER_1_ID=${2}
    local USER_2_ID=${3}

    # Set standard deploy parameters.
    local CHAIN_NAME=$(get_chain_name)
    local GAS_PRICE=${GAS_PRICE:-$NCTL_DEFAULT_GAS_PRICE}
    local GAS_PAYMENT=${GAS_PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
    local NODE_ADDRESS=$(get_node_address_rpc)
    local PATH_TO_CLIENT=$(get_path_to_client)

    # Set contract owner secret key.
    local CONTRACT_OWNER_SECRET_KEY=$(get_path_to_secret_key $NCTL_ACCOUNT_TYPE_FAUCET)

    # Set counter-party 1 account key.
    local USER_1_ACCOUNT_KEY=$(get_account_key $NCTL_ACCOUNT_TYPE_USER $USER_1_ID)

    # Set counter-party 1 account hash.
    local USER_1_ACCOUNT_HASH=$(get_account_hash $USER_1_ACCOUNT_KEY)

    # Set counter-party 2 account key.
    local USER_2_ACCOUNT_KEY=$(get_account_key $NCTL_ACCOUNT_TYPE_USER $USER_2_ID)

    # Set counter-party 2 account hash.
    local USER_2_ACCOUNT_HASH=$(get_account_hash $USER_2_ACCOUNT_KEY)

    # Dispatch transfer (hits node api).
    local DEPLOY_HASH_TRANSFER=$(
        $(get_path_to_client) put-deploy \
            --chain-name $CHAIN_NAME \
            --gas-price $GAS_PRICE \
            --node-address $NODE_ADDRESS \
            --payment-amount $GAS_PAYMENT \
            --secret-key $CONTRACT_OWNER_SECRET_KEY \
            --ttl "1day" \
            --session-name "ERC20" \
            --session-entry-point "transferFrom" \
            --session-arg "$(get_cl_arg_account_hash 'owner' $USER_1_ACCOUNT_HASH)" \
            --session-arg "$(get_cl_arg_account_hash 'recipient' $USER_2_ACCOUNT_HASH)" \
            --session-arg "$(get_cl_arg_u256 'amount' $AMOUNT)" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
    )

    log "ERC20 token transfer"
    log "contract details:"
    log "... arg: owner = $USER_1_ACCOUNT_HASH"
    log "... arg: recipient = $USER_2_ACCOUNT_HASH"
    log "... arg: amount = $AMOUNT"
    log "... entry point = transferFrom"
    log "deploy details:"
    log "... chain = $CHAIN_NAME"
    log "... dispatch node = $NODE_ADDRESS"
    log "... gas payment = $GAS_PAYMENT"
    log "... gas price = $GAS_PRICE"
    log "... hash = $DEPLOY_HASH_TRANSFER"
    log "... signing key = $CONTRACT_OWNER_SECRET_KEY"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset AMOUNT
unset USER_1_ID
unset USER_2_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        user1) USER_1_ID=${VALUE} ;;
        user2) USER_2_ID=${VALUE} ;;        
        *)
    esac
done

main \
    ${AMOUNT:-1000000000} \
    ${USER_1_ID:-1} \
    ${USER_2_ID:-2} 
