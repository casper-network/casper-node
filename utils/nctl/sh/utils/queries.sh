#!/usr/bin/env bash

#######################################
# Returns a timestamp for use in chainspec.toml.
# Arguments:
#   Delay in seconds to apply to genesis timestamp.
#######################################
function get_activation_point()
{
    local DELAY=${1:-0}
    local SCRIPT=(
        "from datetime import datetime, timedelta;"
        "print((datetime.utcnow() + timedelta(seconds=$DELAY)).isoformat('T') + 'Z');"
     )

    python3 -c "${SCRIPT[*]}"
}

#######################################
# Returns a chain era.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_chain_era()
{
    local NODE_ID=${1:-$(get_node_for_dispatch)}

    if [ "$(get_node_is_up "$NODE_ID")" = true ]; then
        $(get_path_to_client) get-block \
            --node-address "$(get_node_address_rpc "$NODE_ID")" \
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

    if [ "$(get_node_is_up "$NODE_ID")" = true ]; then
        $(get_path_to_client) get-block \
            --node-address "$(get_node_address_rpc "$NODE_ID")" \
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

    echo casper-net-"$NET_ID"
}

#######################################
# Returns hash of latest block finalized at a node.
#######################################
function get_chain_latest_block_hash()
{
    local NODE_ID=${1:-$(get_node_for_dispatch)}

    $(get_path_to_client) get-block \
        --node-address "$(get_node_address_rpc "$NODE_ID")" \
        | jq '.result.block.hash' \
        | sed -e 's/^"//' -e 's/"$//'
}

#######################################
# Returns latest block finalized at a node.
#######################################
function get_chain_latest_block()
{
    $(get_path_to_client) get-block \
        --node-address "$(get_node_address_rpc)" \
        | jq '.result.block' \
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
# Returns protocol version formatted for inclusion in chainspec.
# Arguments:
#   Version of protocol.
#######################################
function get_protocol_version_for_chainspec()
{
    local PROTOCOL_VERSION=${1}

    echo "$PROTOCOL_VERSION" | tr "_" "."
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

    $(get_path_to_client) get-state-root-hash \
        --node-address "$(get_node_address_rpc "$NODE_ID")" \
        --block-identifier "${BLOCK_HASH:-""}" \
        | jq '.result.state_root_hash' \
        | sed -e 's/^"//' -e 's/"$//'
}
