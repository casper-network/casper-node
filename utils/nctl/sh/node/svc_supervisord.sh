#!/usr/bin/env bash

#######################################
# Spins up a node using supervisord.
# Arguments:
#   Node ordinal identifier.
#   A trusted hash to streamline joining.
#######################################
function do_node_start()
{
    local NODE_ID=${1}
    local TRUSTED_HASH=${2}

    local PATH_TO_NODE_CONFIG
    local PROCESS_NAME

    if [ ! -e "$(get_path_net_supervisord_sock)" ]; then
        _do_supervisord_start
    fi

    if [ -n "$TRUSTED_HASH" ]; then
        PATH_TO_NODE_CONFIG=$(get_path_to_net)/nodes/node-"$NODE_ID"/config/1_0_0/config.toml
        _update_node_config_on_start "$PATH_TO_NODE_CONFIG" "$TRUSTED_HASH"
    fi

    PROCESS_NAME=$(get_process_name_of_node_in_group "$NODE_ID")
    supervisorctl -c "$(get_path_net_supervisord_cfg)" start "$PROCESS_NAME"  > /dev/null 2>&1
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
    if [ ! -e "$(get_path_net_supervisord_sock)" ]; then
        _do_supervisord_start
    fi

    log "... starting nodes: genesis bootstraps"
    supervisorctl -c "$(get_path_net_supervisord_cfg)" start "$NCTL_PROCESS_GROUP_1":*  > /dev/null 2>&1
    sleep 1.0

    log "... starting nodes: genesis non-bootstraps"
    supervisorctl -c "$(get_path_net_supervisord_cfg)" start "$NCTL_PROCESS_GROUP_2":*  > /dev/null 2>&1
    sleep 1.0
}

#######################################
# Renders to stdout status of a node running under supervisord.
# Arguments:
#   Node ordinal identifier.
#######################################
function do_node_status()
{
    local NODE_ID=${1}
    local NODE_PROCESS_NAME
    
    if [ -e "$(get_path_net_supervisord_sock)" ]; then
        NODE_PROCESS_NAME=$(get_process_name_of_node_in_group "$NODE_ID")
        # True is necessary due to supervisorctl exiting 3
        supervisorctl -c "$(get_path_net_supervisord_cfg)" status "$NODE_PROCESS_NAME" || true
    fi
}

#######################################
# Renders to stdout status of all nodes running under supervisord.
# Arguments:
#   Network ordinal identifier.
#######################################
function do_node_status_all()
{
    if [ -e "$(get_path_net_supervisord_sock)" ]; then
        # True is necessary due to supervisorctl exiting 3
        supervisorctl -c "$(get_path_net_supervisord_cfg)" status all || true
    fi
}

#######################################
# Stops a node running via supervisord.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function do_node_stop()
{
    local NODE_ID=${1}
    local NODE_PROCESS_NAME
    
    if [ -e "$(get_path_net_supervisord_sock)" ]; then
        NODE_PROCESS_NAME=$(get_process_name_of_node_in_group "$NODE_ID")
        supervisorctl -c "$(get_path_net_supervisord_cfg)" stop "$NODE_PROCESS_NAME" > /dev/null 2>&1
    fi
}

#######################################
# Stops all nodes running via supervisord.
#######################################
function do_node_stop_all()
{
    if [ -e "$(get_path_net_supervisord_sock)" ]; then
        log "... stopping supervisord"
        supervisorctl -c "$(get_path_net_supervisord_cfg)" shutdown > /dev/null 2>&1
        sleep 2.0
    fi
}

#######################################
# Starts supervisord (if necessary).
# Arguments:
#   Network ordinal identifier.
#######################################
function _do_supervisord_start()
{
    if [ ! -e "$(get_path_net_supervisord_sock)" ]; then
        log "... starting supervisord"
        supervisord -c "$(get_path_net_supervisord_cfg)"
        sleep 2.0
    fi
}

#######################################
# Sets entry in node's config file.
# Arguments:
#   Node ordinal identifier.
#   A trused block hash from which to build chain state.
#######################################
function _get_node_pid()
{
    local NODE_ID=${1}
    local NODE_PROCESS_NAME
    
    if [ -e "$(get_path_net_supervisord_sock)" ]; then
        NODE_PROCESS_NAME=$(get_process_name_of_node_in_group "$NODE_ID")
        echo $(supervisorctl -c "$(get_path_net_supervisord_cfg)" pid "$NODE_PROCESS_NAME")
    else
        echo "0"
    fi
}

#######################################
# Sets entry in node's config file.
# Arguments:
#   Node ordinal identifier.
#   A trused block hash from which to build chain state.
#######################################
function _update_node_config_on_start()
{
    local FILEPATH=${1}
    local TRUSTED_HASH=${2}
    local SCRIPT
    
    log "trusted hash = $TRUSTED_HASH"

    SCRIPT=(
        "import toml;"
        "cfg=toml.load('$FILEPATH');"
        "cfg['node']['trusted_hash']='$TRUSTED_HASH';"
        "toml.dump(cfg, open('$FILEPATH', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"
}
