#######################################
# Returns a network known addresses - i.e. those of bootstrap nodes.
# Globals:
#   NCTL_BASE_PORT_NETWORK - base network port number.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function get_bootstrap_known_address()
{
    local NODE_ID=${1}
    local NET_ID=${NET_ID:-1}    
    local NODE_PORT=$(($NCTL_BASE_PORT_NETWORK + ($NET_ID * 100) + $NODE_ID))

    echo "127.0.0.1:$NODE_PORT"
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
    echo $(ls -l $(get_path_to_net)/nodes | grep -c ^d)
}

#######################################
# Returns count of test users.
#######################################
function get_count_of_users()
{    
    echo $(ls -l $(get_path_to_net)/users | grep -c ^d)
}

#######################################
# Returns network bind address.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_network_bind_address()
{
    local NODE_ID=${1}

    echo "0.0.0.0:$(get_node_port $NCTL_BASE_PORT_NETWORK $NODE_ID)"   
}

#######################################
# Returns network known addresses.
#######################################
function get_network_known_addresses()
{
    local RESULT=""
    local ADDRESS=""
    for NODE_ID in $(seq 1 $(get_count_of_bootstrap_nodes))
    do
        ADDRESS=$(get_bootstrap_known_address $NODE_ID)
        RESULT=$RESULT$ADDRESS
        if [ $NODE_ID -lt $(get_count_of_bootstrap_nodes) ]; then
            RESULT=$RESULT","
        fi
    done

    echo $RESULT
}


#######################################
# Returns node event address.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_address_event()
{
    local NODE_ID=${1}    

    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_SSE $NODE_ID)"
}

#######################################
# Returns node JSON address.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_address_rest()
{
    local NODE_ID=${1}   

    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_REST $NODE_ID)"
}

#######################################
# Returns node RPC address.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_address_rpc()
{
    local NODE_ID=${1}      

    echo http://localhost:"$(get_node_port $NCTL_BASE_PORT_RPC $NODE_ID)"
}

#######################################
# Returns node RPC address, intended for use with cURL.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_address_rpc_for_curl()
{
    local NODE_ID=${1}   

    echo "$(get_node_address_rpc $NODE_ID)/rpc"
}

#######################################
# Returns ordinal identifier of a validator node able to be used for deploy dispatch.
# Arguments:
#   Network ordinal identifier.
#######################################
function get_node_for_dispatch()
{
    for NODE_ID in $(seq 1 $(get_count_of_nodes) | shuf)
    do
        if [ $(get_node_is_up $NODE_ID) = true ]; then
            echo $NODE_ID
            break
        fi
    done
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
    echo $(($BASE_PORT + ($NET_ID * 100) + $NODE_ID))
}

#######################################
# Calculates REST port.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_port_rest()
{
    local NODE_ID=${1}    

    echo $(get_node_port $NCTL_BASE_PORT_REST $NODE_ID)
}

#######################################
# Calculates RPC port.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_port_rpc()
{
    local NODE_ID=${1}    

    echo $(get_node_port $NCTL_BASE_PORT_RPC $NODE_ID)
}

#######################################
# Calculates SSE port.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_port_sse()
{
    local NODE_ID=${1}    

    echo $(get_node_port $NCTL_BASE_PORT_SSE $NODE_ID)
}

#######################################
# Returns flag indicating whether a node is currently up.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_node_is_up()
{
    local NODE_ID=${1}    
    local NODE_PORT=$(get_node_port_rpc $NODE_ID)

    if grep -q "open" <<< "$(nmap -p $NODE_PORT 127.0.0.1)"; then
        echo true
    else
        echo false
    fi
}

#######################################
# Returns set of nodes within a process group.
# Arguments:
#   Process group identifier.
#######################################
function get_process_group_members()
{
    local PROCESS_GROUP=${1}

    # Set range.
    if [ $PROCESS_GROUP == $NCTL_PROCESS_GROUP_1 ]; then
        local SEQ_START=1
        local SEQ_END=$(get_count_of_bootstrap_nodes)
    elif [ $PROCESS_GROUP == $NCTL_PROCESS_GROUP_2 ]; then
        local SEQ_START=$(($(get_count_of_bootstrap_nodes) + 1))
        local SEQ_END=$(get_count_of_genesis_nodes)
    elif [ $PROCESS_GROUP == $NCTL_PROCESS_GROUP_3 ]; then
        local SEQ_START=$(($(get_count_of_genesis_nodes) + 1))
        local SEQ_END=$(get_count_of_nodes)
    fi

    # Set members of process group.
    local RESULT=""
    for NODE_ID in $(seq $SEQ_START $SEQ_END)
    do
        if [ $NODE_ID -gt $SEQ_START ]; then
            RESULT=$RESULT", "
        fi
        RESULT=$RESULT$(get_process_name_of_node $NODE_ID)
    done

    echo $RESULT
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
# Returns name of a daemonized node process within a group.
# Arguments:
#   Node ordinal identifier.
#######################################
function get_process_name_of_node_in_group()
{
    local NODE_ID=${1} 

    local NODE_PROCESS_NAME=$(get_process_name_of_node $NODE_ID)
    local PROCESS_GROUP_NAME=$(get_process_name_of_node_group $NODE_ID)
    
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
    
    if [ $NODE_ID -le $(get_count_of_bootstrap_nodes) ]; then
        echo $NCTL_PROCESS_GROUP_1
    elif [ $NODE_ID -le $(get_count_of_genesis_nodes) ]; then
        echo $NCTL_PROCESS_GROUP_2
    else
        echo $NCTL_PROCESS_GROUP_3
    fi
}
