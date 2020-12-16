#######################################
# Submits an auction bid.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Validator ordinal identifier.
#   Bid amount.
#   Delegation rate.
#   Gas price.
#   Gas payment.
#######################################
function do_auction_bid_submit()
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local BIDDER_ID=${3}
    local AMOUNT=${4}
    local DELEGATION_RATE=${5}
    local GAS=${6}
    local PAYMENT=${7}
    local QUIET=${8}

    local BIDDER_PUBLIC_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_NODE $BIDDER_ID)
    local BIDDER_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_NODE $BIDDER_ID)
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local PATH_TO_CONTRACT=$(get_path_to_contract $NET_ID "add_bid.wasm")

    if [ $QUIET == "FALSE" ]; then
        log "dispatching deploy -> add_bid.wasm"
        log "... network = $NET_ID"
        log "... node = $NODE_ID"
        log "... node address = $NODE_ADDRESS"
        log "... contract = $PATH_TO_CONTRACT"
        log "... bidder id = $BIDDER_ID"
        log "... bidder secret key = $BIDDER_SECRET_KEY"
        log "... bid amount = $amount"
        log "... bid delegation rate = $DELEGATION_RATE"
    fi

    DEPLOY_HASH=$(
        $(get_path_to_client $NET_ID) put-deploy \
            --chain-name $(get_chain_name $NET_ID) \
            --gas-price $GAS \
            --node-address $NODE_ADDRESS \
            --payment-amount $PAYMENT \
            --secret-key $BIDDER_SECRET_KEY \
            --session-arg="public_key:public_key='$BIDDER_PUBLIC_KEY'" \
            --session-arg "amount:u512='$AMOUNT'" \
            --session-arg "delegation_rate:u64='$DELEGATION_RATE'" \
            --session-path $PATH_TO_CONTRACT \
            --ttl "1day" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )

    if [ $QUIET == "FALSE" ]; then
        log "deploy dispatched:"
        log "... deploy hash = $DEPLOY_HASH"
    fi
}

#######################################
# Submits an auction withdrawal.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Validator ordinal identifier.
#   Withdrawal amount.
#   Gas price.
#   Gas payment.
#######################################
function do_auction_bid_withdraw() 
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local BIDDER_ID=${3}
    local AMOUNT=${4}
    local GAS=${5}
    local PAYMENT=${6}
    local QUIET=${7}

    local BIDDER_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_NODE $BIDDER_ID)
    local BIDDER_ACCOUNT_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_NODE $BIDDER_ID)
    local BIDDER_MAIN_PURSE_UREF=$(get_main_purse_uref $NET_ID $NODE_ID $BIDDER_ACCOUNT_KEY)
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local PATH_TO_CONTRACT=$(get_path_to_contract $NET_ID "withdraw_bid.wasm")

    if [ $QUIET == "FALSE" ]; then
        log "dispatching deploy -> withdraw_bid.wasm"
        log "... network = $NET_ID"
        log "... node = $NODE_ID"
        log "... node address = $NODE_ADDRESS"
        log "... contract = $PATH_TO_CONTRACT"
        log "... bidder id = $BIDDER_ID"
        log "... bidder account key = $BIDDER_ACCOUNT_KEY"
        log "... bidder secret key = $BIDDER_SECRET_KEY"
        log "... bidder main purse uref = $BIDDER_MAIN_PURSE_UREF"
        log "... withdrawal amount = $AMOUNT"
    fi

    DEPLOY_HASH=$(
        $(get_path_to_client $NET_ID) put-deploy \
            --chain-name $(get_chain_name $NET_ID) \
            --gas-price $GAS \
            --node-address $NODE_ADDRESS \
            --payment-amount $PAYMENT \
            --secret-key $BIDDER_SECRET_KEY \
            --session-arg="public_key:public_key='$BIDDER_ACCOUNT_KEY'" \
            --session-arg "amount:u512='$AMOUNT'" \
            --session-arg "unbond_purse:opt_uref='$BIDDER_MAIN_PURSE_UREF'" \
            --session-path $PATH_TO_CONTRACT \
            --ttl "1day" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )

    if [ $QUIET == "FALSE" ]; then
        log "deploy dispatched:"
        log "... deploy hash = $DEPLOY_HASH"
    fi
}

#######################################
# Submits an auction delegate.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Delegator ordinal identifier.
#   Validator ordinal identifier.
#   Amount to delegate.
#   Gas price.
#   Gas payment.
#######################################
function do_auction_delegate_submit()
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local DELEGATOR_ID=${3}
    local VALIDATOR_ID=${4}
    local AMOUNT=${5}
    local GAS=${6}
    local PAYMENT=${7}

    local DELEGATOR_ACCOUNT_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $DELEGATOR_ID)
    local DELEGATOR_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $DELEGATOR_ID)
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local PATH_TO_CONTRACT=$(get_path_to_contract $NET_ID "delegate.wasm")
    local VALIDATOR_ACCOUNT_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_NODE $VALIDATOR_ID)

    log "dispatching deploy -> delegate.wasm"
    log "... network = $NET_ID "
    log "... node address = $NODE_ADDRESS"
    log "... contract = $PATH_TO_CONTRACT"
    log "... delegator id = $DELEGATOR_ID"
    log "... delegator account key = $DELEGATOR_ACCOUNT_KEY"
    log "... delegator secret key = $DELEGATOR_SECRET_KEY"
    log "... amount = $AMOUNT"

    DEPLOY_HASH=$(
        $(get_path_to_client $NET_ID) put-deploy \
            --chain-name $(get_chain_name $NET_ID) \
            --gas-price $GAS \
            --node-address $NODE_ADDRESS \
            --payment-amount $PAYMENT \
            --secret-key $DELEGATOR_SECRET_KEY \
            --session-arg "amount:u512='$AMOUNT'" \
            --session-arg "delegator:public_key='$DELEGATOR_ACCOUNT_KEY'" \
            --session-arg "validator:public_key='$VALIDATOR_ACCOUNT_KEY'" \
            --session-path $PATH_TO_CONTRACT \
            --ttl "1day" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )

    log "deploy dispatched:"
    log "... deploy hash = $DEPLOY_HASH"
}

#######################################
# Submits an auction delegate withdrawal.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Delegator ordinal identifier.
#   Validator ordinal identifier.
#   Amount to delegate.
#   Gas price.
#   Gas payment.
#######################################
function do_auction_delegate_withdraw()
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local DELEGATOR_ID=${3}
    local VALIDATOR_ID=${4}
    local AMOUNT=${5}
    local GAS=${6}
    local PAYMENT=${7}

    local DELEGATOR_ACCOUNT_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $DELEGATOR_ID)
    local DELEGATOR_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $DELEGATOR_ID)
    local DELEGATOR_MAIN_PURSE_UREF=$(get_main_purse_uref $NET_ID $NODE_ID $DELEGATOR_ACCOUNT_KEY)
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local PATH_TO_CONTRACT=$(get_path_to_contract $NET_ID "undelegate.wasm")
    local VALIDATOR_PUBLIC_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_NODE $VALIDATOR_ID)

    log "dispatching deploy -> undelegate.wasm"
    log "... network = $NET_ID"
    log "... node = $NODE_ID"
    log "... node address = $NODE_ADDRESS"
    log "... contract = $PATH_TO_CONTRACT"
    log "... delegator id = $DELEGATOR_ID"
    log "... delegator account key = $DELEGATOR_ACCOUNT_KEY"
    log "... delegator secret key = $DELEGATOR_SECRET_KEY"
    log "... delegator main purse uref = $DELEGATOR_MAIN_PURSE_UREF"
    log "... amount = $AMOUNT"

    DEPLOY_HASH=$(
        $(get_path_to_client $NET_ID) put-deploy \
            --chain-name $(get_chain_name $NET_ID) \
            --gas-price $GAS \
            --node-address $NODE_ADDRESS \
            --payment-amount $PAYMENT \
            --secret-key $DELEGATOR_SECRET_KEY \
            --session-arg "amount:u512='$AMOUNT'" \
            --session-arg "delegator:public_key='$DELEGATOR_ACCOUNT_KEY'" \
            --session-arg "validator:public_key='$VALIDATOR_PUBLIC_KEY'" \
            --session-arg "unbond_purse:opt_uref='$DELEGATOR_MAIN_PURSE_UREF'" \
            --session-path $PATH_TO_CONTRACT \
            --ttl "1day" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )

    log "deploy dispatched:"
    log "... deploy hash = $DEPLOY_HASH"
}
