#######################################
# Renders an account.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Dispatch node ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account() {
    local NET_ID=${1}
    local NODE_ID=${2}      
    local ACCOUNT_TYPE=${3}
    local ACCOUNT_IDX=${4}   

    local ACCOUNT_KEY=$(get_account_key $NET_ID $ACCOUNT_TYPE $ACCOUNT_IDX)
    local STATE_ROOT_HASH=$(get_state_root_hash $NET_ID $NODE_ID)

    source $NCTL/sh/views/view_chain_account.sh \
        net=$NET_ID \
        node=$NODE_ID \
        root-hash=$STATE_ROOT_HASH \
        account-key=$ACCOUNT_KEY
}

#######################################
# Renders an account balance.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Dispatch node ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_balance() {
    local NET_ID=${1}
    local NODE_ID=${2}      
    local ACCOUNT_TYPE=${3}
    local ACCOUNT_IDX=${4} 
    
    local ACCOUNT_KEY=$(get_account_key $NET_ID $ACCOUNT_TYPE $ACCOUNT_IDX)
    local ACCOUNT_PREFIX=$(get_account_prefix $NET_ID $ACCOUNT_TYPE $ACCOUNT_IDX)
    local STATE_ROOT_HASH=$(get_state_root_hash $NET_ID $NODE_ID)
    local PURSE_UREF=$(get_main_purse_uref $NET_ID $NODE_ID $ACCOUNT_KEY $STATE_ROOT_HASH)

    source $NCTL/sh/views/view_chain_account_balance.sh \
        net=$NET_ID \
        node=$NODE_ID \
        root-hash=$STATE_ROOT_HASH \
        purse-uref=$PURSE_UREF \
        prefix=$ACCOUNT_PREFIX
}

#######################################
# Renders an account hash.
# Arguments:
#   Network ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_hash() {
    local NET_ID=${1}
    local ACCOUNT_TYPE=${2}
    local ACCOUNT_IDX=${3}   

    local ACCOUNT_KEY=$(get_account_key $net $ACCOUNT_TYPE $ACCOUNT_IDX)
    local ACCOUNT_HASH=$(get_account_hash $ACCOUNT_KEY)
    local ACCOUNT_PREFIX=$(get_account_prefix $net $ACCOUNT_TYPE $ACCOUNT_IDX)

    log "$ACCOUNT_PREFIX.account-hash = $ACCOUNT_HASH"
}

#######################################
# Renders an account key.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Network ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_key() {
    local NET_ID=${1}
    local ACCOUNT_TYPE=${2}
    local ACCOUNT_IDX=${3}  

    local ACCOUNT_KEY=$(get_account_key $net $ACCOUNT_TYPE $ACCOUNT_IDX)
    local ACCOUNT_PREFIX=$(get_account_prefix $net $ACCOUNT_TYPE $ACCOUNT_IDX)

    log "$ACCOUNT_PREFIX.account-key = $ACCOUNT_KEY"
}

#######################################
# Renders an account's main purse uref.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_main_purse_uref() {
    local NET_ID=${1}
    local NODE_ID=${2}
    local ACCOUNT_TYPE=${3}
    local ACCOUNT_IDX=${4}  
    local STATE_ROOT_HASH=${5:-$(get_state_root_hash $NET_ID $NODE_ID)}

    local ACCOUNT_KEY=$(get_account_key $NET_ID $ACCOUNT_TYPE $ACCOUNT_IDX)
    local ACCOUNT_PREFIX=$(get_account_prefix $NET_ID $ACCOUNT_TYPE $ACCOUNT_IDX)
    local PURSE_UREF=$(get_main_purse_uref $NET_ID $NODE_ID $ACCOUNT_KEY $STATE_ROOT_HASH)

    log "$ACCOUNT_PREFIX.main-purse-uref = $PURSE_UREF"
}

#######################################
# Renders an account secret key path.
# Globals:
#   NCTL_ACCOUNT_TYPE_FAUCET - faucet account type.
#   NCTL_ACCOUNT_TYPE_NODE - node account type.
#   NCTL_ACCOUNT_TYPE_USER - user account type.
# Arguments:
#   Network ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_account_secret_key() {
    local NET_ID=${1}
    local ACCOUNT_TYPE=${2}
    local ACCOUNT_IDX=${3}    

    local ACCOUNT_PREFIX=$(get_account_prefix $NET_ID $ACCOUNT_TYPE $ACCOUNT_IDX)
    local PATH_TO_KEY=$(get_path_to_secret_key $NET_ID $ACCOUNT_TYPE $ACCOUNT_IDX)

    log "$ACCOUNT_PREFIX.secret-key-path = $PATH_TO_KEY"
}

#######################################
# Renders on-chain block transfer information.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_chain_block_transfers() {
    local NET_ID=${1}
    local NODE_ID=${2}
    local BLOCK_HASH=${3}      
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)

    if [ "$BLOCK_HASH" ]; then
        $(get_path_to_client $NET_ID) get-block \
            --node-address $NODE_ADDRESS \
            --block-identifier $BLOCK_HASH \
            | jq '.result.block'
    else
        $(get_path_to_client $NET_ID) get-block \
            --node-address $NODE_ADDRESS \
            | jq '.result.block'
    fi
}

#######################################
# Renders on-chain auction information.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_chain_auction_info() {
    local NET_ID=${1}
    local NODE_ID=${2}

    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)

    $(get_path_to_client $NET_ID) get-auction-info \
        --node-address $NODE_ADDRESS \
        | jq '.result'
}

#######################################
# Renders on-chain block information.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_chain_block() {
    local NET_ID=${1}
    local NODE_ID=${2}
    local BLOCK_HASH=${3}      
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)

    if [ "$BLOCK_HASH" ]; then
        $(get_path_to_client $NET_ID) get-block \
            --node-address $NODE_ADDRESS \
            --block-identifier $BLOCK_HASH \
            | jq '.result.block'
    else
        $(get_path_to_client $NET_ID) get-block \
            --node-address $NODE_ADDRESS \
            | jq '.result.block'
    fi
}

#######################################
# Renders on-chain deploy information.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Account type (node | user | faucet).
#   Account ordinal identifier (optional).
#######################################
function render_chain_deploy() {
    local NET_ID=${1}
    local NODE_ID=${2}
    local DEPLOY_HASH=${3}     

    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)

    $(get_path_to_client $NET_ID) get-deploy \
        --node-address $NODE_ADDRESS \
        $DEPLOY_HASH \
        | jq '.result'
}

#######################################
# Renders on-chain era information.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_chain_era_info() {
    local NET_ID=${1}
    local NODE_ID=${2}

    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)

    $(get_path_to_client $NET_ID) get-era-info \
        --node-address $NODE_ADDRESS \
        --block-identifier "" \
        | jq '.result'
}

#######################################
# Renders a state root hash at a certain node.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   User ordinal identifier.
#######################################
function render_chain_state_root_hash() {
    local NET_ID=${1}
    local NODE_ID=${2}
    local BLOCK_HASH=${3}

    local NODE_IS_UP=$(get_node_is_up $NET_ID $NODE_ID)
    if [ "$NODE_IS_UP" = true ]; then
        local STATE_ROOT_HASH=$(get_state_root_hash $NET_ID $NODE_ID $BLOCK_HASH)
    fi

    log "state root hash @ net-$NET_ID.node-$NODE_ID = "${STATE_ROOT_HASH:-'N/A'}
}

#######################################
# Displays to stdout current node metrics.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Metric name.
#######################################
function render_node_metrics() {
    local NET_ID=${1}
    local NODE_ID=${2}
    local METRICS=${3}

    local ENDPOINT=$(get_node_address_rest $NET_ID $NODE_ID)/metrics

    if [ $METRICS = "all" ]; then
        curl -s --location --request GET $ENDPOINT  
    else
        echo "network #$NET_ID :: node #$NODE_ID :: "$(curl -s --location --request GET $ENDPOINT | grep $METRICS | tail -n 1)
    fi
}

#######################################
# Displays to stdout current node peers.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_node_peers() {
    local NET_ID=${1}
    local NODE_ID=${2}
    
    local NODE_ADDRESS_CURL=$(get_node_address_rpc_for_curl $NET_ID $NODE_ID)
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
        log "net #$NET_ID :: node #$NODE_ID :: peers: N/A"
    else
        log "net #$NET_ID :: node #$NODE_ID :: peers:"
        echo $NODE_API_RESPONSE | jq
    fi
}

#######################################
# Displays to stdout current node ports.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_node_ports() {
    local NET_ID=${1}
    local NODE_ID=${2}

    local PORT_VNET=$(get_node_port $NCTL_BASE_PORT_NETWORK $NET_ID $NODE_ID)
    local PORT_REST=$(get_node_port_rest $NET_ID $NODE_ID)
    local PORT_RPC=$(get_node_port_rpc $NET_ID $NODE_ID)
    local PORT_SSE=$(get_node_port_sse $NET_ID $NODE_ID)

    log "net #$NET_ID :: node #$NODE_ID :: VNET @ $PORT_VNET :: RPC @ $PORT_RPC :: REST @ $PORT_REST :: SSE @ $PORT_SSE"
}

#######################################
# Displays to stdout current node status.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_node_rpc_schema() {
    local NET_ID=${1}
    local NODE_ID=${2}
    
    local NODE_ADDRESS=$(get_node_address_rpc $NET_ID $NODE_ID)
    local NODE_ADDRESS_CURL=$(get_node_address_rpc_for_curl $NET_ID $NODE_ID)

    log "net #$NET_ID :: node #$NODE_ID :: $NODE_ADDRESS :: rpc schema:"

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
function render_node_status() {
    local NET_ID=${1}
    local NODE_ID=${2}
    
    local NODE_ADDRESS_CURL=$(get_node_address_rpc_for_curl $NET_ID $NODE_ID)
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
        log "net #$NET_ID :: node #$NODE_ID :: status: N/A"
    else
        log "net #$NET_ID :: node #$NODE_ID :: status:"
        echo $NODE_API_RESPONSE | jq
    fi
}

#######################################
# Displays to stdout current node storage stats.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function render_node_storage() {
    local NET_ID=${1}
    local NODE_ID=${2}
    
    local OS_TYPE="$(get_os)"
    local PATH_TO_NODE_STORAGE=$(get_path_to_node $NET_ID $NODE_ID)/storage/*

    log "net #$NET_ID :: node #$NODE_ID :: storage stats:"

    if [[ $OS_TYPE == $_OS_LINUX* ]]; then
        ll $PATH_TO_NODE_STORAGE
    elif [[ $OS_TYPE == $_OS_MACOSX ]]; then
        ls -lG $PATH_TO_NODE_STORAGE
    fi
}
