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
    local CONFIG_TOML_ARRAY
    local PROCESS_NAME

    PATH_TO_NODE_CONFIG="$(get_path_to_node_config $NODE_ID)"
    CONFIG_TOML_ARRAY=($(find "$PATH_TO_NODE_CONFIG" -name 'config.toml'))

    if [ ! -e "$(get_path_net_supervisord_sock)" ]; then
        _do_supervisord_start
    fi

    if [ -n "$TRUSTED_HASH" ]; then
        for i in ${CONFIG_TOML_ARRAY[@]}; do
            _update_node_config_on_start "$i" "$TRUSTED_HASH"
        done
    fi

    PROCESS_NAME=$(get_process_name_of_node_in_group "$NODE_ID")
    supervisorctl -c "$(get_path_net_supervisord_cfg)" start "$PROCESS_NAME"  > /dev/null 2>&1

    PROCESS_NAME=$(get_process_name_of_sidecar_in_group "$NODE_ID")
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
    local SIDECAR_PROCESS_NAME
    
    if [ -e "$(get_path_net_supervisord_sock)" ]; then
        SIDECAR_PROCESS_NAME=$(get_process_name_of_sidecar_in_group "$NODE_ID")
        supervisorctl -c "$(get_path_net_supervisord_cfg)" stop "$SIDECAR_PROCESS_NAME" > /dev/null 2>&1

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
        supervisorctl -c "$(get_path_net_supervisord_cfg)" shutdown > /dev/null 2>&1 || true
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
