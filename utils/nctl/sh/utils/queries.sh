#!/usr/bin/env bash

#######################################
# Returns a chain era or -2 if not available.
# Arguments:
#   Node ordinal identifier.
#   Duration for which to retry once per second in the case the node is not responding.
#######################################
function get_chain_era()
{
    local NODE_ID=${1}
    local TIMEOUT_SEC=${2:-20}
    local ERA

    ERA=$(_get_from_status_with_retry "$NODE_ID" "$TIMEOUT_SEC" ".last_added_block_info.era_id")
    if [[ -z "$ERA" ]]; then
        echo -2
    else
        echo "$ERA"
    fi
}

#######################################
# Returns a chain height or "N/A" if not available.
# Arguments:
#   Node ordinal identifier.
#   Duration for which to retry once per second in the case the node is not responding.
#######################################
function get_chain_height()
{
    local NODE_ID=${1}
    local TIMEOUT_SEC=${2:-20}
    local HEIGHT

    HEIGHT=$(_get_from_status_with_retry "$NODE_ID" "$TIMEOUT_SEC" ".last_added_block_info.height")
    if [[ -z "$HEIGHT" ]]; then
        echo "N/A"
    else
        echo "$HEIGHT"
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
# Arguments:
#   Node ordinal identifier.
#   Duration for which to retry once per second in the case the node is not responding.
#######################################
function get_chain_latest_block_hash()
{
    local NODE_ID=${1}
    local TIMEOUT_SEC=${2:-20}

    echo $(_get_from_status_with_retry "$NODE_ID" "$TIMEOUT_SEC" ".last_added_block_info.hash") \
        | tr '[:upper:]' '[:lower:]' \
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
# Returns protocol version being run by a node.
# Arguments:
#   Node ordinal identifier.
#   Duration for which to retry once per second in the case the node is not responding.
#######################################
function get_node_protocol_version()
{
    local NODE_ID=${1}
    local TIMEOUT_SEC=${2:-20}

    echo $(_get_from_status_with_retry "$NODE_ID" "$TIMEOUT_SEC" ".api_version") \
        | sed -e 's/^"//' -e 's/"$//'
}

#######################################
# Returns the lowest complete block the node has.
# Arguments:
#   Node ordinal identifier.
#   Duration for which to retry once per second in the case the node is not responding.
#######################################
function get_node_lowest_available_block()
{
    local NODE_ID=${1}
    local TIMEOUT_SEC=${2:-20}

    echo $(_get_from_status_with_retry "$NODE_ID" "$TIMEOUT_SEC" ".available_block_range.low") \
        | sed -e 's/^"//' -e 's/"$//'
}

#######################################
# Returns the highest complete block the node has.
# Arguments:
#   Node ordinal identifier.
#   Duration for which to retry once per second in the case the node is not responding.
#######################################
function get_node_highest_available_block()
{
    local NODE_ID=${1}
    local TIMEOUT_SEC=${2:-20}

    echo $(_get_from_status_with_retry "$NODE_ID" "$TIMEOUT_SEC" ".available_block_range.high") \
        | sed -e 's/^"//' -e 's/"$//'
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
    local USE_LATEST=${3:-false}
    local PATH_TO_NODE_BIN=$(get_path_to_node_bin "$NODE_ID")
    local IFS='_'

    pushd "$PATH_TO_NODE_BIN" || exit
    read -ra SEMVER_CURRENT <<< "$(ls --group-directories-first -tdr -- * | head -n 1)"
    popd || exit

    echo "${SEMVER_CURRENT[0]}$SEPARATOR${SEMVER_CURRENT[1]}$SEPARATOR${SEMVER_CURRENT[2]}"
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
# Arguments:
#   Node ordinal identifier.
#   Duration for which to retry once per second in the case the node is not responding.
#######################################
function get_node_connected_peer_count()
{
    local NODE_ID=${1}
    local TIMEOUT_SEC=${2:-20}

    _get_from_status_with_retry "$NODE_ID" "$TIMEOUT_SEC" ".peers" | jq length
}

#######################################
# Returns the given field from the /status response of the given node, or an empty string if unavailable or "null".
# Arguments:
#   Node ordinal identifier.
#   Duration for which to retry once per second in the case the node is not responding.
#   String to be passed to `jq` to fetch the required field.
#######################################
function _get_from_status_with_retry()
{
    local INITIAL_NODE_ID=${1}
    local TIMEOUT_SEC=${2}
    local JQ_STRING=${3}
    local NODE_ID
    local ATTEMPTS=0
    local OUTPUT

    while [ "$ATTEMPTS" -le "$TIMEOUT_SEC" ]; do
        NODE_ID=${INITIAL_NODE_ID:-$(get_node_for_dispatch)}
        if [ $(get_node_is_up "$NODE_ID") = true ]; then
            OUTPUT=$(curl $NCTL_CURL_ARGS_FOR_NODE_RELATED_QUERIES "$(get_node_address_rest $NODE_ID)/status" | jq "$JQ_STRING")
        fi
        if [[ -n "$OUTPUT" ]] && [ "$OUTPUT" != "null" ]; then
            echo "$OUTPUT"
            return
        fi
        ATTEMPTS=$((ATTEMPTS + 1))
        if [ "$ATTEMPTS" -lt "$TIMEOUT_SEC" ]; then
            sleep 1
        fi
    done
    echo ""
}

