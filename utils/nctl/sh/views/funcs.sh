#######################################
# Renders an account.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account()
{
    local ACCOUNT_TYPE=${1}
    local ACCOUNT_IDX=${2}   

    local ACCOUNT_KEY=$(get_account_key $ACCOUNT_TYPE $ACCOUNT_IDX)
    local STATE_ROOT_HASH=$(get_state_root_hash)

    source $NCTL/sh/views/view_chain_account.sh \
        root-hash=$STATE_ROOT_HASH \
        account-key=$ACCOUNT_KEY
}

#######################################
# Renders an account balance.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_balance()
{
    local ACCOUNT_TYPE=${1}
    local ACCOUNT_IDX=${2} 
    
    local ACCOUNT_KEY=$(get_account_key $ACCOUNT_TYPE $ACCOUNT_IDX)
    local ACCOUNT_PREFIX=$(get_account_prefix $ACCOUNT_TYPE $ACCOUNT_IDX)
    local STATE_ROOT_HASH=$(get_state_root_hash)
    local PURSE_UREF=$(get_main_purse_uref $ACCOUNT_KEY $STATE_ROOT_HASH)

    source $NCTL/sh/views/view_chain_balance.sh \
        root-hash=$STATE_ROOT_HASH \
        purse-uref=$PURSE_UREF \
        prefix=$ACCOUNT_PREFIX
}

#######################################
# Renders an account hash.
# Arguments:
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_hash()
{
    local ACCOUNT_TYPE=${1}
    local ACCOUNT_IDX=${2}   

    local ACCOUNT_KEY=$(get_account_key $ACCOUNT_TYPE $ACCOUNT_IDX)
    local ACCOUNT_HASH=$(get_account_hash $ACCOUNT_KEY)
    local ACCOUNT_PREFIX=$(get_account_prefix $ACCOUNT_TYPE $ACCOUNT_IDX)

    log "$ACCOUNT_PREFIX.account-hash = $ACCOUNT_HASH"
}

#######################################
# Renders an account key.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_key()
{
    local ACCOUNT_TYPE=${1}
    local ACCOUNT_IDX=${2}  

    local ACCOUNT_KEY=$(get_account_key $ACCOUNT_TYPE $ACCOUNT_IDX)
    local ACCOUNT_PREFIX=$(get_account_prefix $ACCOUNT_TYPE $ACCOUNT_IDX)

    log "$ACCOUNT_PREFIX.account-key = $ACCOUNT_KEY"
}

#######################################
# Renders an account's main purse uref.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#   State root hash (optional).
#######################################
function render_account_main_purse_uref()
{
    local ACCOUNT_TYPE=${1}
    local ACCOUNT_IDX=${2}  
    local STATE_ROOT_HASH=${3:-$(get_state_root_hash)}

    local ACCOUNT_KEY=$(get_account_key $ACCOUNT_TYPE $ACCOUNT_IDX)
    local ACCOUNT_PREFIX=$(get_account_prefix $ACCOUNT_TYPE $ACCOUNT_IDX)
    local PURSE_UREF=$(get_main_purse_uref $ACCOUNT_KEY $STATE_ROOT_HASH)

    log "$ACCOUNT_PREFIX.main-purse-uref = $PURSE_UREF"
}

#######################################
# Renders an account secret key path.
# Arguments:
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_secret_key()
{
    local ACCOUNT_TYPE=${1}
    local ACCOUNT_IDX=${2}    

    local ACCOUNT_PREFIX=$(get_account_prefix $ACCOUNT_TYPE $ACCOUNT_IDX)
    local PATH_TO_KEY=$(get_path_to_secret_key $ACCOUNT_TYPE $ACCOUNT_IDX)

    log "$ACCOUNT_PREFIX.secret-key-path = $PATH_TO_KEY"
}

#######################################
# Renders on-chain auction information.
#######################################
function render_chain_auction_info()
{
    local NODE_ADDRESS=$(get_node_address_rpc)

    $(get_path_to_client) get-auction-info \
        --node-address $NODE_ADDRESS \
        | jq '.result'
}

#######################################
# Renders on-chain block information.
# Arguments:
#   Block hash.
#######################################
function render_chain_block()
{
    local BLOCK_HASH=${1}

    local NODE_ADDRESS=$(get_node_address_rpc)

    if [ "$BLOCK_HASH" ]; then
        $(get_path_to_client) get-block \
            --node-address $NODE_ADDRESS \
            --block-identifier $BLOCK_HASH \
            | jq '.result.block'
    else
        $(get_path_to_client) get-block \
            --node-address $NODE_ADDRESS \
            | jq '.result.block'
    fi
}

#######################################
# Renders on-chain block transfer information.
# Arguments:
#   Block hash.
#######################################
function render_chain_block_transfers()
{
    local BLOCK_HASH=${1}
    local NODE_ADDRESS=$(get_node_address_rpc)

    if [ "$BLOCK_HASH" ]; then
        $(get_path_to_client) get-block \
            --node-address $NODE_ADDRESS \
            --block-identifier $BLOCK_HASH \
            | jq '.result.block'
    else
        $(get_path_to_client) get-block \
            --node-address $NODE_ADDRESS \
            | jq '.result.block'
    fi
}

#######################################
# Renders on-chain deploy information.
# Arguments:
#   Deploy hash.
#######################################
function render_chain_deploy()
{
    local DEPLOY_HASH=${1}
    local NODE_ADDRESS=$(get_node_address_rpc)

    $(get_path_to_client) get-deploy \
        --node-address $NODE_ADDRESS \
        $DEPLOY_HASH \
        | jq '.result'
}

#######################################
# Renders on-chain era information.
# Arguments:
#   Node ordinal identifier (optional).
#######################################
function render_chain_era_info()
{
    local NODE_ID=${1}
    local NODE_ADDRESS=$(get_node_address_rpc $NODE_ID)

    $(get_path_to_client) get-era-info-by-switch-block \
        --node-address $NODE_ADDRESS \
        --block-identifier "" \
        | jq '.result'
}

#######################################
# Renders a state root hash at a certain node.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Node ordinal identifier.
#   Hash of block at which to return associated state root hash.
#######################################
function render_chain_state_root_hash()
{
    local NODE_ID=${1}
    local BLOCK_HASH=${2}

    local NODE_IS_UP=$(get_node_is_up $NODE_ID)
    if [ "$NODE_IS_UP" = true ]; then
        local STATE_ROOT_HASH=$(get_state_root_hash $NODE_ID $BLOCK_HASH)
    fi

    log "state root hash @ node-$NODE_ID = "${STATE_ROOT_HASH:-'N/A'}
}

#######################################
# Displays to stdout current node metrics.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Metric name.
#######################################
function render_node_metrics()
{
    local NODE_ID=${1}
    local METRICS=${2}

    local ENDPOINT=$(get_node_address_rest $NODE_ID)/metrics

    if [ $METRICS = "all" ]; then
        curl -s --location --request GET $ENDPOINT  
    else
        echo "node #$NODE_ID :: "$(curl -s --location --request GET $ENDPOINT | grep $METRICS | tail -n 1)
    fi
}

#######################################
# Displays to stdout current node peers.
# Arguments:
#   Node ordinal identifier.
#######################################
function render_node_peers()
{
    local NODE_ID=${1}
    
    local NODE_ADDRESS_CURL=$(get_node_address_rpc_for_curl $NODE_ID)
    local NODE_API_RESPONSE=$(
        curl -s --header 'Content-Type: application/json' \
            --request POST $NODE_ADDRESS_CURL \
            --data-raw '{
                "id": 1,
                "jsonrpc": "2.0",
                "method": "info_get_peers"
            }' | jq '.result.peers'
    )

    if [ -z "$NODE_API_RESPONSE" ]; then
        log "node #$NODE_ID :: peers: N/A"
    else
        log "node #$NODE_ID :: peers:"
        echo $NODE_API_RESPONSE | jq
    fi
}

#######################################
# Displays to stdout current node ports.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Node ordinal identifier.
#######################################
function render_node_ports()
{
    local NODE_ID=${1}

    local PORT_VNET=$(get_node_port $NCTL_BASE_PORT_NETWORK $NODE_ID)
    local PORT_REST=$(get_node_port_rest $NODE_ID)
    local PORT_RPC=$(get_node_port_rpc $NODE_ID)
    local PORT_SSE=$(get_node_port_sse $NODE_ID)

    log "node-$NODE_ID :: VNET @ $PORT_VNET :: RPC @ $PORT_RPC :: REST @ $PORT_REST :: SSE @ $PORT_SSE"
}

#######################################
# Displays to stdout RPC schema.
#######################################
function render_node_rpc_schema()
{
    local NODE_ADDRESS_CURL=$(get_node_address_rpc_for_curl)

    curl -s --header 'Content-Type: application/json' \
        --request POST $NODE_ADDRESS_CURL \
        --data-raw '{
            "id": 1,
            "jsonrpc": "2.0",
            "method": "rpc.discover"
        }' | jq '.result'
}

#######################################
# Displays to stdout current node status.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_node_status()
{
    local NODE_ID=${1}
    
    local NODE_ADDRESS_CURL=$(get_node_address_rpc_for_curl $NODE_ID)
    local NODE_API_RESPONSE=$(
        curl -s --header 'Content-Type: application/json' \
            --request POST $NODE_ADDRESS_CURL \
            --data-raw '{
                "id": 1,
                "jsonrpc": "2.0",
                "method": "info_get_status"
            }' | jq '.result'
    )

    if [ -z "$NODE_API_RESPONSE" ]; then
        log "node #$NODE_ID :: status: N/A"
    else
        log "node #$NODE_ID :: status:"
        echo $NODE_API_RESPONSE | jq
    fi
}

#######################################
# Displays to stdout current node storage stats.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Node ordinal identifier.
#######################################
function render_node_storage()
{
    local NODE_ID=${1}
    
    local OS_TYPE="$(get_os)"
    local PATH_TO_NODE_STORAGE=$(get_path_to_node $NODE_ID)/storage/*

    log "node #$NODE_ID :: storage stats:"

    if [[ $OS_TYPE == $_OS_LINUX* ]]; then
        ll $PATH_TO_NODE_STORAGE
    elif [[ $OS_TYPE == $_OS_MACOSX ]]; then
        ls -lG $PATH_TO_NODE_STORAGE
    fi
}
