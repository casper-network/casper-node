#!/usr/bin/env bash
#
# Set of supervisord node process control functions.
# Globals:
#   NCTL - path to nctl home directory.

# Import utils.
source $NCTL/sh/utils.sh

#######################################
# Spins up a node using supervisord.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function do_node_start()
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local NODE_PROCESS_UNIT=$(get_node_process_name $NET_ID $NODE_ID)

    # Ensure daemon is up.
    do_supervisord_start $NET_ID
    
    # Signal to supervisorctl.
    supervisorctl -c "$(get_path_net_supervisord_cfg $NET_ID)" start "$NODE_PROCESS_UNIT"  > /dev/null 2>&1
}

#######################################
# Spins up all nodes using supervisord.
# Arguments:
#   Network ordinal identifier.
#   Count of nodes within network.
#   Count of bootstraps within network.
#######################################
function do_node_start_all()
{
    local NET_ID=${1}
    local NET_NODE_COUNT=${2}
    local NET_BOOTSTRAP_COUNT=${3}
        
    # Step 1: start bootstraps.
    log "net-$NET_ID: starting bootstraps ... "
    for IDX in $(seq 1 $NET_NODE_COUNT)
    do
        if [ $IDX -le $NET_BOOTSTRAP_COUNT ]; then
            log "net-$NET_ID: bootstrapping node $IDX"
            do_node_start $NET_ID $IDX
        fi
    done
    sleep 1.0

    # Step 2: start non-bootstraps.
    log "net-$NET_ID: starting non-bootstraps... "
    supervisorctl -c "$(get_path_net_supervisord_cfg $NET_ID)" start all  > /dev/null 2>&1
}

#######################################
# Renders to stdout status of a node running under supervisord.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function do_node_status()
{
    local NET_ID=${1}
    local NODE_ID=${2}

    # Ensure daemon is up.
    do_supervisord_start $NET_ID

    # Signal to supervisorctl.
    supervisorctl -c "$(get_path_net_supervisord_cfg $NET_ID)" status $(get_node_process_name $NET_ID $NODE_ID)
}

#######################################
# Renders to stdout status of all nodes running under supervisord.
# Arguments:
#   Network ordinal identifier.
#######################################
function do_node_status_all()
{
    local NET_ID=${1}

    # Ensure daemon is up.
    do_supervisord_start $NET_ID

    # Signal to supervisorctl.
    supervisorctl -c "$(get_path_net_supervisord_cfg $NET_ID)" status all
}

#######################################
# Stops a node running via supervisord.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function do_node_stop()
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local NODE_PROCESS_UNIT=$(get_node_process_name $NET_ID $NODE_ID)

    # Ensure daemon is up.
    do_supervisord_start $NET_ID
    
    # Signal to supervisorctl.
    supervisorctl -c "$(get_path_net_supervisord_cfg $NET_ID)" stop "$NODE_PROCESS_UNIT"  > /dev/null 2>&1
}

#######################################
# Stops all nodes running via supervisord.
# Arguments:
#   Network ordinal identifier.
#######################################
function do_node_stop_all()
{
    local NET_ID=${1}

    # Ensure daemon is up.
    do_supervisord_start $NET_ID
    
    # Signal to supervisorctl.
    supervisorctl -c "$(get_path_net_supervisord_cfg $NET_ID)" stop all  > /dev/null 2>&1
}

#######################################
# Starts supervisord (if necessary).
# Arguments:
#   Network ordinal identifier.
#######################################
function do_supervisord_start()
{
    local NET_ID=${1}

    # If sock file not found then start daemon.
    if [ ! -e "$(get_path_net_supervisord_sock $NET_ID)" ]; then
        supervisord -c "$(get_path_net_supervisord_cfg $NET_ID)"
        sleep 2.0
    fi
}

#######################################
# Kills supervisord (if necessary).
# Arguments:
#   Network ordinal identifier.
#######################################
function do_supervisord_kill()
{
    local NET_ID=${1}

    # If sock file exists then stop daemon.
    if [ -e "$(get_path_net_supervisord_sock $NET_ID)" ]; then
        supervisorctl -c "$(get_path_net_supervisord_cfg $NET_ID)" stop all &>/dev/null
        supervisorctl -c "$(get_path_net_supervisord_cfg $NET_ID)" shutdown &>/dev/null
    fi
}
