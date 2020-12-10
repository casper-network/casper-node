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
    local NODE_PROCESS_NAME=$(get_process_name_of_node_in_group $NET_ID $NODE_ID)
    local PATH_TO_NODE_CONFIG=$PATH_NET/nodes/node-$NODE_ID/config/node-config.toml

    # Import net vars.
    source $(get_path_to_net_vars $NET_ID)

    # Ensure daemon is up.
    do_supervisord_start $NET_ID

    # Inject most recent trusted hash.
    if [ $NODE_ID -gt $NCTL_NET_NODE_COUNT ]; then
        local TRUSTED_HASH=$(get_chain_latest_block_hash $NET_ID)
        sed -i "s/#trusted_hash/trusted_hash/g" $PATH_TO_NODE_CONFIG > /dev/null 2>&1
        sed -i "s/^\(trusted_hash\) = .*/\1 = \'${TRUSTED_HASH}\'/" $PATH_TO_NODE_CONFIG > /dev/null 2>&1
    fi

    # Signal to supervisorctl.
    supervisorctl -c "$(get_path_net_supervisord_cfg $NET_ID)" start "$NODE_PROCESS_NAME"  > /dev/null 2>&1
}

#######################################
# Spins up a node using supervisord.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function do_node_start_group()
{
    local NET_ID=${1}
    local GROUP_ID=${2}

    # Ensure daemon is up.
    do_supervisord_start $NET_ID

    # Signal to supervisorctl.
    supervisorctl -c "$(get_path_net_supervisord_cfg $NET_ID)" start "$GROUP_ID":*  > /dev/null 2>&1
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
    log "net-$NET_ID: starting genesis bootstraps ... "
    do_node_start_group $NET_ID $NCTL_PROCESS_GROUP_1
    sleep 1.0

    # Step 2: start non-bootstraps.
    log "net-$NET_ID: starting genesis non-bootstraps... "
    do_node_start_group $NET_ID $NCTL_PROCESS_GROUP_2
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
    local NODE_PROCESS_NAME=$(get_process_name_of_node_in_group $NET_ID $NODE_ID)

    # Ensure daemon is up.
    do_supervisord_start $NET_ID

    # Signal to supervisorctl.
    supervisorctl -c "$(get_path_net_supervisord_cfg $NET_ID)" status "$NODE_PROCESS_NAME"
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
    local NODE_PROCESS_NAME=$(get_process_name_of_node_in_group $NET_ID $NODE_ID)

    # Ensure daemon is up.
    do_supervisord_start $NET_ID
    
    # Signal to supervisorctl.
    supervisorctl -c "$(get_path_net_supervisord_cfg $NET_ID)" stop "$NODE_PROCESS_NAME"  > /dev/null 2>&1
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
