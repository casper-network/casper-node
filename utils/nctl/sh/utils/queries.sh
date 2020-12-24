#######################################
# Returns a chain era.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_chain_era()
{
    local NODE_ID=${1:-$(get_node_for_dispatch)}
    local NODE_ADDRESS=$(get_node_address_rpc $NODE_ID)

    if [ $(get_node_is_up $NODE_ID) = true ]; then
        $(get_path_to_client) get-block \
            --node-address $NODE_ADDRESS \
            --block-identifier "" \
            | jq '.result.block.header.era_id'    
    else
        echo "N/A"
    fi
}

#######################################
# Returns a chain height.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_chain_height()
{
    local NODE_ID=${1:-$(get_node_for_dispatch)}
    local NODE_ADDRESS=$(get_node_address_rpc $NODE_ID)

    if [ $(get_node_is_up $NODE_ID) = true ]; then
        $(get_path_to_client) get-block \
            --node-address $NODE_ADDRESS \
            --block-identifier "" \
            | jq '.result.block.header.height'    
    else
        echo "N/A"
    fi
}

#######################################
# Returns a chain name.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_chain_name()
{
    local NET_ID=${NET_ID:-1}

    echo casper-net-$NET_ID
}

#######################################
# Returns latest block finalized at a node.
#######################################
function get_chain_latest_block_hash()
{
    local NODE_ADDRESS=$(get_node_address_rpc)

    $(get_path_to_client) get-block \
        --node-address $NODE_ADDRESS \
        | jq '.result.block.hash' \
        | sed -e 's/^"//' -e 's/"$//'
}

#######################################
# Returns a timestamp for use in chainspec.toml.
# Arguments:
#   Delay in seconds to apply to genesis timestamp.
#######################################
function get_genesis_timestamp()
{
    local DELAY=${1}
    local SCRIPT=(
        "from datetime import datetime, timedelta;"
        "print((datetime.utcnow() + timedelta(seconds=$DELAY)).isoformat('T') + 'Z');"
     )

    python3 -c "${SCRIPT[*]}"
}

#######################################
# Returns a state root hash.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Node ordinal identifier.
#   Block identifier.
#######################################
function get_state_root_hash()
{
    local NODE_ID=${1} 
    local BLOCK_HASH=${2}

    local NODE_ADDRESS=$(get_node_address_rpc $NODE_ID)

    $(get_path_to_client) get-state-root-hash \
        --node-address $NODE_ADDRESS \
        --block-identifier ${BLOCK_HASH:-""} \
        | jq '.result.state_root_hash' \
        | sed -e 's/^"//' -e 's/"$//'
}