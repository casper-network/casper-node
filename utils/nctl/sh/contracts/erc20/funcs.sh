#######################################
# Approves a token transfer by a user for a specific amount.
# Arguments:
#   Amount of ERC-20 token to permit transfer.
#   User ordinal identifier.
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Gas payment.
#   Gas price.
#######################################
function do_erc20_approve()
{
    local AMOUNT=$((${1} * 2))
    local USER_ID=${2}
    local NET_ID=${3}
    local NODE_ID=${4}
    local GAS_PAYMENT=${5}
    local GAS_PRICE=${6}

    local CONTRACT_OWNER_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_FAUCET)
    local USER_ACCOUNT_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $USER_ID)
    local USER_ACCOUNT_HASH=$(get_account_hash $USER_ACCOUNT_KEY)

    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local CHAIN_NAME=$(get_chain_name $NET_ID)

    local DEPLOY_HASH=$(
        $(get_path_to_client $NET_ID) put-deploy \
            --chain-name $CHAIN_NAME \
            --gas-price $GAS_PRICE \
            --node-address $NODE_ADDRESS \
            --payment-amount $GAS_PAYMENT \
            --secret-key $CONTRACT_OWNER_SECRET_KEY \
            --ttl "1day" \
            --session-name "ERC20" \
            --session-entry-point "approve" \
            --session-arg "spender:account_hash='account-hash-$USER_ACCOUNT_HASH'" \
            --session-arg "amount:U256='$AMOUNT'" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
    )

    log "approving ERC20 token transfer"
    log "... network = "$NET_ID
    log "... node = "$NODE_ID
    log "... node address = "$NODE_ADDRESS
    log "... user account key = "$USER_ACCOUNT_KEY
    log "... approved amount = "$AMOUNT
    log "... deploy hash = "$DEPLOY_HASH
}

#######################################
# ERC-20: get on-chain contract hash.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Contract owner account key.
#######################################
function get_erc20_contract_hash () {
    local NET_ID=${1}
    local NODE_ID=${2}
    local ACCOUNT_KEY=${3}

    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local STATE_ROOT_HASH=$(get_state_root_hash $NET_ID $NODE_ID)

    echo $(
        $(get_path_to_client $NET_ID) query-state \
            --node-address $NODE_ADDRESS \
            --state-root-hash $STATE_ROOT_HASH \
            --key $ACCOUNT_KEY \
            | jq '.result.stored_value.Account.named_keys.ERC20.Hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )
}

#######################################
# ERC-20: get on-chain contract key value.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Contract owner account key.
#   State query path.
#######################################
function get_erc20_contract_key_value () {
    local NET_ID=${1}
    local NODE_ID=${2}
    local ACCOUNT_KEY=${3}
    local QUERY_PATH=${4}

    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local STATE_ROOT_HASH=$(get_state_root_hash $NET_ID $NODE_ID)

    echo $(
        $(get_path_to_client $NET_ID) query-state \
            --node-address $NODE_ADDRESS \
            --state-root-hash $STATE_ROOT_HASH \
            --key $ACCOUNT_KEY \
            --query-path $QUERY_PATH \
            | jq '.result.stored_value.CLValue.parsed_to_json' \
            | sed -e 's/^"//' -e 's/"$//'
        )
}

#######################################
# Transfers ERC-20 tokens from contract owner to test user accounts.
# Arguments:
#   Amount of ERC-20 token to transfer.
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Gas payment.
#   Gas price.
#######################################
function do_erc20_fund_users()
{
    local AMOUNT=$((${1} * 2))
    local NET_ID=${2}
    local NODE_ID=${3}
    local GAS_PAYMENT=${4}
    local GAS_PRICE=${5}

    local CONTRACT_OWNER_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_FAUCET)
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local CHAIN_NAME=$(get_chain_name $NET_ID)

    for USER_ID in $(seq 1 $(get_count_of_users $NET_ID))
    do
        local USER_ACCOUNT_KEY=$(get_account_key $net $NCTL_ACCOUNT_TYPE_USER $USER_ID)
        local USER_ACCOUNT_HASH=$(get_account_hash $USER_ACCOUNT_KEY)

        local DEPLOY_HASH=$(
            $(get_path_to_client $NET_ID) put-deploy \
                --chain-name $CHAIN_NAME \
                --gas-price $GAS_PRICE \
                --node-address $NODE_ADDRESS \
                --payment-amount $GAS_PAYMENT \
                --secret-key $CONTRACT_OWNER_SECRET_KEY \
                --ttl "1day" \
                --session-name "ERC20" \
                --session-entry-point "transfer" \
                --session-arg "recipient:account_hash='account-hash-$USER_ACCOUNT_HASH'" \
                --session-arg "amount:U256='$amount'" \
                | jq '.result.deploy_hash' \
                | sed -e 's/^"//' -e 's/"$//'
        )

        log "funding user $USER_ID deploy hash = "$DEPLOY_HASH
    done
}

#######################################
# Installs ERC-20 token contract under network faucet account.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Name of ERC-20 token being created.
#   Symbol associated with ERC-20 token.
#   Total supply of ERC-20 token.
#   Gas payment.
#   Gas price.
#######################################
function do_erc20_install_contract()
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local TOKEN_NAME=${3}
    local TOKEN_SYMBOL=${4}
    local TOKEN_SUPPLY=${5}
    local GAS_PAYMENT=${6}
    local GAS_PRICE=${7}

    local CONTRACT_OWNER_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_FAUCET)
    local CONTRACT_PATH=$(get_path_to_contract $NET_ID "erc20.wasm")
    if [ ! -f $CONTRACT_PATH ]; then
        echo "ERROR: The erc20.wasm binary file cannot be found.  Please compile it and move it to the following directory: "$(get_path_to_net $NET_ID)
        return
    fi

    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local CHAIN_NAME=$(get_chain_name $NET_ID)

    local DEPLOY_HASH=$(
        $(get_path_to_client $NET_ID) put-deploy \
            --chain-name $CHAIN_NAME \
            --gas-price $GAS_PRICE \
            --node-address $NODE_ADDRESS \
            --payment-amount $GAS_PAYMENT \
            --secret-key $CONTRACT_OWNER_SECRET_KEY \
            --session-path $CONTRACT_PATH \
            --session-arg "tokenName:string='$TOKEN_NAME'" \
            --session-arg "tokenSymbol:string='$TOKEN_SYMBOL'" \
            --session-arg "tokenTotalSupply:u256='$TOKEN_SUPPLY'" \
            --ttl "1day" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
        )

    log "installing contract -> ERC-20"
    log "... network = $NET_ID"
    log "... node = $NODE_ID"
    log "... node address = "$NODE_ADDRESS
    log "contract constructor args:"
    log "... token name = "$TOKEN_NAME
    log "... token symbol = "$TOKEN_SYMBOL
    log "... token supply = "$TOKEN_SUPPLY
    log "contract installation details:"
    log "... path = "$CONTRACT_PATH
    log "... deploy hash = "$DEPLOY_HASH
}

#######################################
# Transfers ERC-20 tokens from one user to another - i.e. peer to peer transfer.
# Arguments:
#   Amount of ERC-20 token to transfer.
#   User 1 ordinal identifier.
#   User 2 ordinal identifier.
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Gas payment.
#   Gas price.
#######################################
function do_erc20_transfer()
{
    local AMOUNT=$((${1} * 2))
    local USER_1_ID=${2}
    local USER_2_ID=${3}
    local NET_ID=${4}
    local NODE_ID=${5}
    local GAS_PAYMENT=${6}
    local GAS_PRICE=${7}

    local CONTRACT_OWNER_SECRET_KEY=$(get_path_to_secret_key $NET_ID $NCTL_ACCOUNT_TYPE_FAUCET)
    local USER_1_ACCOUNT_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $USER_1_ID)
    local USER_1_ACCOUNT_HASH=$(get_account_hash $USER_1_ACCOUNT_KEY)
    local USER_2_ACCOUNT_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_USER $USER_2_ID)
    local USER_2_ACCOUNT_HASH=$(get_account_hash $USER_2_ACCOUNT_KEY)

    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local CHAIN_NAME=$(get_chain_name $NET_ID)

    local DEPLOY_HASH_APPROVAL=$(
        $(get_path_to_client $NET_ID) put-deploy \
            --chain-name $CHAIN_NAME \
            --gas-price $GAS_PRICE \
            --node-address $NODE_ADDRESS \
            --payment-amount $GAS_PAYMENT \
            --secret-key $CONTRACT_OWNER_SECRET_KEY \
            --ttl "1day" \
            --session-name "ERC20" \
            --session-entry-point "approve" \
            --session-arg "spender:account_hash='account-hash-$USER_1_ACCOUNT_HASH'" \
            --session-arg "amount:U256='$AMOUNT'" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
    )

    # TODO: better way to await finalisation via a while loop.
    sleep 5.0

    local DEPLOY_HASH_TRANSFER=$(
        $(get_path_to_client $NET_ID) put-deploy \
            --chain-name $CHAIN_NAME \
            --gas-price $GAS_PRICE \
            --node-address $NODE_ADDRESS \
            --payment-amount $GAS_PAYMENT \
            --secret-key $CONTRACT_OWNER_SECRET_KEY \
            --ttl "1day" \
            --session-name "ERC20" \
            --session-entry-point "transferFrom" \
            --session-arg "owner:account_hash='account-hash-$USER_1_ACCOUNT_HASH'" \
            --session-arg "recipient:account_hash='account-hash-$USER_2_ACCOUNT_HASH'" \
            --session-arg "amount:U256='$AMOUNT'" \
            | jq '.result.deploy_hash' \
            | sed -e 's/^"//' -e 's/"$//'
    )

    log "transfering ERC20 tokens"
    log "... network = $NET_ID"
    log "... node = $NODE_ID"
    log "... node address = "$NODE_ADDRESS
    log "... user 1 account key = "$USER_1_ACCOUNT_KEY
    log "... user 2 account key = "$USER_2_ACCOUNT_KEY
    log "... deploy hash - approval = "$DEPLOY_HASH_APPROVAL
    log "... deploy hash - transfer = "$DEPLOY_HASH_TRANSFER
}


#######################################
# Renders ERC-20 token contract balances.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_erc20_contract_balances()
{
    local NET_ID=${1}
    local NODE_ID=${2}
    
    # Set contract owner account key - i.e. faucet account.
    local CONTRACT_OWNER_ACCOUNT_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_FAUCET)

    # Set contract owner account hash.
    local CONTRACT_OWNER_ACCOUNT_HASH=$(get_account_hash $CONTRACT_OWNER_ACCOUNT_KEY)

    # Set contract hash.
    local CONTRACT_HASH=$(get_erc20_contract_hash $NET_ID $NODE_ID $CONTRACT_OWNER_ACCOUNT_KEY)

    # Set token symbol.
    local TOKEN_SYMBOL=$(get_erc20_contract_key_value $NET_ID $NODE_ID $CONTRACT_HASH "_symbol")

    # Set contract owner account balance.
    CONTRACT_OWNER_ACCOUNT_BALANCE=$(
        get_erc20_contract_key_value $NET_ID $NODE_ID $CONTRACT_HASH "_balances_"$CONTRACT_OWNER_ACCOUNT_HASH
        )

    # Render contract owner account balance.
    log "ERC-20 $TOKEN_SYMBOL Account Balances:"
    log "... contract owner = "$CONTRACT_OWNER_ACCOUNT_BALANCE

    # Render user account balances.
    for USER_ID in $(seq 1 $(get_count_of_users $NET_ID))
    do
        # Set user account key.
        local USER_ACCOUNT_KEY=$(get_account_key $net $NCTL_ACCOUNT_TYPE_USER $USER_ID)

        # Set user account hash.
        local USER_ACCOUNT_HASH=$(get_account_hash $USER_ACCOUNT_KEY)

        # Set user account balance.
        local USER_ACCOUNT_BALANCE_KEY="_balances_"$USER_ACCOUNT_HASH
        local USER_ACCOUNT_BALANCE=$(get_erc20_contract_key_value $NET_ID $NODE_ID $CONTRACT_HASH $USER_ACCOUNT_BALANCE_KEY)

        # Render user account balance - note adjust for U256 - U512 rounding.
        log "... user $USER_ID = "$(($USER_ACCOUNT_BALANCE * 2))
    done
}

#######################################
# Renders ERC-20 token contract details.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_erc20_contract_details()
{
    local NET_ID=${1}
    local NODE_ID=${2}

    # Set contract owner account key - i.e. faucet account.
    local CONTRACT_OWNER_ACCOUNT_KEY=$(get_account_key $NET_ID $NCTL_ACCOUNT_TYPE_FAUCET)

    # Set contract hash.
    local CONTRACT_HASH=$(get_erc20_contract_hash $NET_ID $NODE_ID $CONTRACT_OWNER_ACCOUNT_KEY)

    # Set token name.
    local TOKEN_NAME=$(get_erc20_contract_key_value $NET_ID $NODE_ID $CONTRACT_HASH "_name")

    # Set token symbol.
    local TOKEN_SYMBOL=$(get_erc20_contract_key_value $NET_ID $NODE_ID $CONTRACT_HASH "_symbol")

    # Set token supply.
    local TOKEN_SUPPLY=$(get_erc20_contract_key_value $NET_ID $NODE_ID $CONTRACT_HASH "_totalSupply")

    # Set token decimals.
    local TOKEN_DECIMALS=$(get_erc20_contract_key_value $NET_ID $NODE_ID $CONTRACT_HASH "_decimals")

    # Render details.
    log "Contract Details -> ERC-20"
    log "... on-chain name = ERC20"
    log "... on-chain hash = "$CONTRACT_HASH
    log "... owner account = "$CONTRACT_OWNER_ACCOUNT_KEY
    log "... token name = "$TOKEN_NAME
    log "... token symbol = "$TOKEN_SYMBOL
    log "... token supply = "$TOKEN_SUPPLY
    log "... token decimals = "$TOKEN_DECIMALS
}
