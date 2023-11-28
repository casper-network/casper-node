#!/usr/bin/env bash

#######################################
# Returns a bootstrap known address - i.e. those of bootstrap nodes.
# Globals:
#   NCTL_BASE_PORT_NETWORK - base network port number.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_bootstrap_known_address()
{
    local NODE_ID=${1}
    local NET_ID=${NET_ID:-1}
    local NODE_PORT=$((NCTL_BASE_PORT_NETWORK + (NET_ID * 100) + NODE_ID))

    echo "'127.0.0.1:$NODE_PORT'"
}

#######################################
# Returns count of a network's bootstrap nodes.
#######################################
function get_count_of_bootstrap_nodes()
{
    # Hard-coded.
    echo 3
}

#######################################
# Returns count of a network's bootstrap nodes.
#######################################
function get_count_of_genesis_nodes()
{
    echo $(($(get_count_of_nodes) / 2))
}

#######################################
# Returns count of all network nodes.
#######################################
function get_count_of_nodes()
{
    find "$(get_path_to_net)"/nodes/* -maxdepth 0 -type d | wc -l
}

#######################################
# Returns count of currently up nodes.
#######################################
function get_count_of_up_nodes()
{
    local COUNT=0
    local NODE_ID

    for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
    do
        if [ "$(get_node_is_up "$NODE_ID")" == true ]; then
            COUNT=$((COUNT + 1))
        fi
    done

    echo $COUNT
}

#######################################
# Returns count of test users.
#######################################
function get_count_of_users()
{
    find "$(get_path_to_net)"/users/* -maxdepth 0 -type d | wc -l
}

#######################################
# Returns network bind address.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_network_bind_address()
{
    local NODE_ID=${1}

    echo "0.0.0.0:$(get_node_port "$NCTL_BASE_PORT_NETWORK" "$NODE_ID")"
}

#######################################
# Returns network known addresses.
#######################################
function get_network_known_addresses()
{
    local NODE_ID=${1}
    local RESULT

    # If a bootstrap node then return set of bootstraps.
    RESULT=$(get_bootstrap_known_address 1)
    if [ "$NODE_ID" -lt "$(get_count_of_bootstrap_nodes)" ]; then
        for IDX in $(seq 2 "$(get_count_of_bootstrap_nodes)")
        do
            RESULT=$RESULT","$(get_bootstrap_known_address "$IDX")
        done
    # If a non-bootstrap node then return full set of nodes.
    # Note: could be modified to return full set of spinning nodes.
    else
        for IDX in $(seq 2 "$NODE_ID")
        do
            RESULT=$RESULT","$(get_bootstrap_known_address "$IDX")
        done
    fi

    echo "$RESULT"
}

#######################################
# Returns node event address.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_address_event()
{
    local NODE_ID=${1}

    echo "http://localhost:$(get_node_port "$NCTL_BASE_PORT_SSE" "$NODE_ID")"
}

#######################################
# Returns node JSON address.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_address_rest()
{
    local NODE_ID=${1}

    echo "http://localhost:$(get_node_port "$NCTL_BASE_PORT_REST" "$NODE_ID")"
}

#######################################
# Returns node RPC address.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_address_rpc()
{
    local NODE_ID=${1}

    echo "http://localhost:$(get_node_port "$NCTL_BASE_PORT_RPC" "$NODE_ID")"
}

#######################################
# Returns node RPC address, intended for use with cURL.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_address_rpc_for_curl()
{
    local NODE_ID=${1}

    echo "$(get_node_address_rpc "$NODE_ID")/rpc"
}

#######################################
# Returns ordinal identifier of a random validator node able to be used for deploy dispatch.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_node_for_dispatch()
{
    for NODE_ID in $(seq 1 "$(get_count_of_nodes)" | shuf)
    do
        if [ "$(get_node_is_up "$NODE_ID")" = true ]; then
            echo "$NODE_ID"
            break
        fi
    done
}

#######################################
# Returns flag indicating whether a node is currently up.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_is_up()
{
    local NODE_ID=${1}
    local NODE_PORT

    NODE_PORT=$(get_node_port_rpc "$NODE_ID")

    if grep -q "$NODE_PORT (LISTEN)" <<< "$(lsof -i -P -n)"; then
        echo true
    else
        echo false
    fi
}

#######################################
# Calculate port for a given base port, network id, and node id.
# Arguments:
#   Base starting port.
#   Node ordinal identifier.
#######################################
function get_node_port()
{
    local BASE_PORT=${1}
    local NODE_ID=${2:-$(get_node_for_dispatch)}
    local NET_ID=${NET_ID:-1}

    # TODO: Need to handle case of more than 99 nodes.
    echo $((BASE_PORT + (NET_ID * 100) + NODE_ID))
}

#######################################
# Calculates binary port.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_port_binary()
{
    local NODE_ID=${1}

    get_node_port "$NCTL_BASE_PORT_BINARY" "$NODE_ID"
}

#######################################
# Calculates REST port.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_port_rest()
{
    local NODE_ID=${1}

    get_node_port "$NCTL_BASE_PORT_REST" "$NODE_ID"
}

#######################################
# Calculates RPC port.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_port_rpc()
{
    local NODE_ID=${1}

    get_node_port "$NCTL_BASE_PORT_RPC" "$NODE_ID"
}

#######################################
# Calculates speculative exec port.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_port_speculative_exec()
{
    local NODE_ID=${1}

    get_node_port "$NCTL_BASE_PORT_SPEC_EXEC" "$NODE_ID"
}

#######################################
# Calculates SSE port.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_port_sse()
{
    local NODE_ID=${1}

    get_node_port "$NCTL_BASE_PORT_SSE" "$NODE_ID"
}

#######################################
# Calculates a node's default staking weight.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_staking_weight()
{
    local NODE_ID=${1}

    echo $((NCTL_VALIDATOR_BASE_WEIGHT + NODE_ID))
}

#######################################
# Returns set of nodes within a process group.
# Arguments:
#   Process group identifier.
#######################################
function get_process_group_members()
{
    local PROCESS_GROUP=${1}
    local SEQ_END
    local SEQ_START

    # Set range.
    if [ "$PROCESS_GROUP" == "$NCTL_PROCESS_GROUP_1" ]; then
        SEQ_START=1
        SEQ_END=$(get_count_of_bootstrap_nodes)

    elif [ "$PROCESS_GROUP" == "$NCTL_PROCESS_GROUP_2" ]; then
        SEQ_START=$(($(get_count_of_bootstrap_nodes) + 1))
        SEQ_END=$(get_count_of_genesis_nodes)

    elif [ "$PROCESS_GROUP" == "$NCTL_PROCESS_GROUP_3" ]; then
        SEQ_START=$(($(get_count_of_genesis_nodes) + 1))
        SEQ_END=$(get_count_of_nodes)
    fi

    # Set members of process group.
    local RESULT=""
    for NODE_ID in $(seq "$SEQ_START" "$SEQ_END")
    do
        if [ "$NODE_ID" -gt "$SEQ_START" ]; then
            RESULT=$RESULT", "
        fi
        RESULT=$RESULT$(get_process_name_of_node "$NODE_ID"),$(get_process_name_of_sidecar "$NODE_ID")
    done

    echo "$RESULT"
}

#######################################
# Returns name of a daemonized node process within a group.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_process_name_of_node()
{
    local NODE_ID=${1}
    local NET_ID=${NET_ID:-1}

    echo "casper-net-$NET_ID-node-$NODE_ID"
}

#######################################
# Returns name of a daemonized sidecar process within a group.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_process_name_of_sidecar()
{
    local NODE_ID=${1}
    local NET_ID=${NET_ID:-1}

    echo "casper-net-$NET_ID-sidecar-$NODE_ID"
}

#######################################
# Returns name of a daemonized node process within a group.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_process_name_of_node_in_group()
{
    local NODE_ID=${1}
    local NODE_PROCESS_NAME
    local PROCESS_GROUP_NAME

    NODE_PROCESS_NAME=$(get_process_name_of_node "$NODE_ID")
    PROCESS_GROUP_NAME=$(get_process_name_of_node_group "$NODE_ID")

    echo "$PROCESS_GROUP_NAME:$NODE_PROCESS_NAME"
}

#######################################
# Returns name of a daemonized node process within a group.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_process_name_of_sidecar_in_group()
{
    local NODE_ID=${1}
    local NODE_PROCESS_NAME
    local PROCESS_GROUP_NAME

    NODE_PROCESS_NAME=$(get_process_name_of_sidecar "$NODE_ID")
    PROCESS_GROUP_NAME=$(get_process_name_of_node_group "$NODE_ID")

    echo "$PROCESS_GROUP_NAME:$NODE_PROCESS_NAME"
}

#######################################
# Returns name of a daemonized node process group.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_process_name_of_node_group()
{
    local NODE_ID=${1}

    if [ "$NODE_ID" -le "$(get_count_of_bootstrap_nodes)" ]; then
        echo "$NCTL_PROCESS_GROUP_1"
    elif [ "$NODE_ID" -le "$(get_count_of_genesis_nodes)" ]; then
        echo "$NCTL_PROCESS_GROUP_2"
    else
        echo "$NCTL_PROCESS_GROUP_3"
    fi
}

#######################################
# Returns count of nodes that atleast attempted to start
#######################################
function get_count_of_started_nodes()
{
    nctl-status | grep -v 'Not started' | wc -l
}


#######################################
# Returns only if the chain has reached genesis (interpreted as era>=2)
#######################################
function do_await_genesis_era_to_complete() {
    local LOG_STEP=${1:-'true'}
    local TIMEOUT=${2:-'240'}
    local CURRENT_ERA

    if [ "$LOG_STEP" = "true" ]; then
        log_step "awaiting genesis era to complete: timeout=$TIMEOUT"
    fi

    while :
    do
        CURRENT_ERA=$(get_chain_era)
        if [ "$CURRENT_ERA" -ge "1" ]
        then
            log "genesis reached, era=$CURRENT_ERA"
            return
        fi
        TIMEOUT=$((TIMEOUT-1))
        if [ "$TIMEOUT" = '0' ]; then
            log "ERROR: Timed out before genesis era completed"
            exit 1
        else
            log "... waiting for genesis era to complete: timeout=$TIMEOUT, current era=$CURRENT_ERA"
        fi
        sleep 1.0
    done
}

