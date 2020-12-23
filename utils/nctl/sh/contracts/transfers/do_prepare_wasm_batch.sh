#!/usr/bin/env bash

source $NCTL/sh/utils.sh

#######################################
# Prepares wasm transfers for dispatch to a test net.
# Arguments:
#   Transfer amount.
#   Count of transfer batches to be dispatched.
#   Size of transfer batches to be dispatched.
#######################################
function main()
{
    local AMOUNT=${1}
    local BATCH_COUNT=${2}
    local BATCH_SIZE=${3}

    # Set standard deploy parameters.
    local CHAIN_NAME=$(get_chain_name)
    local GAS_PRICE=${GAS_PRICE:-$NCTL_DEFAULT_GAS_PRICE}
    local GAS_PAYMENT=${GAS_PAYMENT:-$NCTL_DEFAULT_GAS_PAYMENT}
    local PATH_TO_CLIENT=$(get_path_to_client)
    local PATH_TO_CONTRACT=$(get_path_to_contract "transfer_to_account_u512.wasm")
    local PATH_TO_NET=$(get_path_to_net)    

    # Set counter-party 1 secret key.
    local CP1_SECRET_KEY=$(get_path_to_secret_key $NCTL_ACCOUNT_TYPE_FAUCET)

    # Remove existing.
    if [ -d $PATH_TO_NET/deploys/transfer-wasm ]; then
        rm -rf $PATH_TO_NET/deploys/transfer-wasm
    fi

    # Enumerate set of users.
    for USER_ID in $(seq 1 $(get_count_of_users))
    do
        # Set counter-party 2 account key.
        local CP2_ACCOUNT_KEY=$(get_account_key $NCTL_ACCOUNT_TYPE_USER $USER_ID)

        # Set counter-party 2 account hash.
        local CP2_ACCOUNT_HASH=$(get_account_hash $CP2_ACCOUNT_KEY)

        # Enumerate set of batches.
        for BATCH_ID in $(seq 1 $BATCH_COUNT)
        do
            # Set path to output.
            local PATH_TO_OUTPUT=$PATH_TO_NET/deploys/transfer-wasm/batch-$BATCH_ID/user-$USER_ID
            mkdir -p $PATH_TO_OUTPUT

            # Enumerate set of transfer to prepare.
            for TRANSFER_ID in $(seq 1 $BATCH_SIZE)
            do
                # Set unsigned deploy.
                local PATH_TO_OUTPUT_UNSIGNED=$PATH_TO_OUTPUT/transfer-$TRANSFER_ID-unsigned.json
                $PATH_TO_CLIENT make-deploy \
                    --output $PATH_TO_OUTPUT_UNSIGNED \
                    --chain-name $CHAIN_NAME \
                    --gas-price $GAS_PRICE \
                    --payment-amount $GAS_PAYMENT \
                    --ttl "1day" \
                    --secret-key $CP1_SECRET_KEY \
                    --session-arg "$(get_cl_arg_u512 'amount' $AMOUNT)" \
                    --session-arg "$(get_cl_arg_account_hash 'target' $CP2_ACCOUNT_HASH)" \
                    --session-path $PATH_TO_CONTRACT 

                # Set signed deploy.
                local PATH_TO_OUTPUT_SIGNED=$PATH_TO_OUTPUT/transfer-$TRANSFER_ID.json
                $PATH_TO_CLIENT sign-deploy \
                    --secret-key $CP1_SECRET_KEY \
                    --input $PATH_TO_OUTPUT_UNSIGNED \
                    --output $PATH_TO_OUTPUT_SIGNED
                
                # Tidy up.
                rm $PATH_TO_OUTPUT_UNSIGNED
            done
        done
    done
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset AMOUNT
unset BATCH_COUNT
unset BATCH_SIZE

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        amount) AMOUNT=${VALUE} ;;
        count) BATCH_COUNT=${VALUE} ;;
        size) BATCH_SIZE=${VALUE} ;;
        *)
    esac
done

main \
    ${AMOUNT:-$NCTL_DEFAULT_TRANSFER_AMOUNT} \
    ${BATCH_COUNT:-5} \
    ${BATCH_SIZE:-200} 
