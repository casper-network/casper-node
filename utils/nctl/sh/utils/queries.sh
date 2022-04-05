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
    local ERA

    if [ "$(get_node_is_up "$NODE_ID")" = true ]; then
        ERA=$(curl -s "$(get_node_address_rest $NODE_ID)/status" | jq '.last_added_block_info.era_id')
        if [ "$ERA" == "null" ]; then
            echo 0
        else
            echo $ERA
        fi
    else
        echo -1
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
        curl -s "$(get_node_address_rest $NODE_ID)/status" | jq '.last_added_block_info.height'
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

    curl -s "$(get_node_address_rest $NODE_ID)/status" \
    | jq '.last_added_block_info.hash' \
    | tr '[:upper:]' '[:lower:]' \
    | sed -e 's/^"//' -e 's/"$//'
}

function get_chain_first_block_hash()
{
    local NODE_ID=${1:-$(get_node_for_dispatch)}

    $(get_path_to_client) get-block \
        --node-address "$(get_node_address_rpc "$NODE_ID")" \
        -b '0' \
        | jq '.result.block.hash' \
        | sed -e 's/^"//' -e 's/"$//'
}

#######################################
# Returns latest block finalized at a node.
#######################################
function get_chain_latest_block()
{
    local NODE_ID=${1:-$(get_node_for_dispatch)}

    $(get_path_to_client) get-block \
        --node-address "$(get_node_address_rpc $NODE_ID)" \
        | jq '.result.block'
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
function get_node_status()
{
    local NODE_ID=${1}
    local NODE_ADDRESS_CURL
    local NODE_API_RESPONSE

    NODE_ADDRESS_CURL=$(get_node_address_rpc_for_curl "$NODE_ID")
    NODE_API_RESPONSE=$(
        curl -s --header 'Content-Type: application/json' \
            --request POST "$NODE_ADDRESS_CURL" \
            --data-raw '{
                "id": 1,
                "jsonrpc": "2.0",
                "method": "info_get_status"
            }'
    )

    echo $NODE_API_RESPONSE | jq '.result'
}

#######################################
# Returns protocol version being run by a node.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_protocol_version()
{
    local NODE_ID=${1}

    echo $(get_node_status "$NODE_ID") | \
         jq '.api_version' | \
         sed -e 's/^"//' -e 's/"$//'
}

#######################################
# Returns protocol version being run by a node by inspecting it's bin folder.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_protocol_version_from_fs()
{
    local NODE_ID=${1}
    local SEPARATOR=${2:-"."}
    local PATH_TO_NODE_BIN=$(get_path_to_node_bin "$NODE_ID")
    local IFS='_'

    pushd "$PATH_TO_NODE_BIN" || exit
    read -ra SEMVAR_CURRENT <<< "$(ls --group-directories-first -tdr -- * | head -n 1)"
    popd || exit

    echo "${SEMVAR_CURRENT[0]}$SEPARATOR${SEMVAR_CURRENT[1]}$SEPARATOR${SEMVAR_CURRENT[2]}"
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
# Returns protocol version formatted for filesystem usage.
# Arguments:
#   Version of protocol.
#######################################
function get_protocol_version_for_fs()
{
    local PROTOCOL_VERSION=${1}

    echo "$PROTOCOL_VERSION" | tr "." "_"
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
    local BLOCK_ID=${2:-""}

    $(get_path_to_client) get-state-root-hash \
        --node-address "$(get_node_address_rpc "$NODE_ID")" \
        --block-identifier "$BLOCK_ID" \
        | jq '.result.state_root_hash' \
        | sed -e 's/^"//' -e 's/"$//'
}

#######################################
# Returns the number of connected peers.
#######################################
function get_node_connected_peer_count()
{
    local NODE_ID=${1}

    echo $(curl -s "$(get_node_address_rest $NODE_ID)/status" | jq '.peers' | jq length)
}

