#!/usr/bin/env bash

#######################################
# Spins up a node using systemd.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Count of bootstraps within network.
#   Trusted hash (optional).
#######################################
function do_node_start()
{
    local NODE_ID=${1}
    local TRUSTED_HASH=${2:-}
    local PATH_TO_NET
    local PATH_TO_NET_CHAINSPEC
    local PATH_TO_NODE
    local PATH_TO_NODE_CONFIG
    local PATH_TO_NODE_SECRET_KEY
    local PATH_TO_NODE_LOG_FILE_STDOUT
    local PATH_TO_NODE_LOG_FILE_STDERR
    local PORT_NODE_API_REST
    local PORT_NODE_API_RPC
    local PORT_NODE_API_EVENT
    local NETWORK_BIND_ADDRESS
    local NETWORK_KNOWN_ADDRESSES
    
    # Set net asset paths.
    PATH_TO_NET=$(get_path_to_net)
    PATH_TO_NET_CHAINSPEC=$PATH_TO_NET/chainspec/chainspec.toml

    # Set node asset paths.
    PATH_TO_NODE=$(get_path_to_node "$NODE_ID")
    PATH_TO_NODE_CONFIG=$PATH_TO_NODE/config/config.toml
    PATH_TO_NODE_SECRET_KEY=$PATH_TO_NODE/keys/secret_key.pem
    PATH_TO_NODE_STORAGE=$PATH_TO_NODE/storage
    PATH_TO_NODE_LOG_FILE_STDOUT=$PATH_TO_NODE/logs/stdout.log
    PATH_TO_NODE_LOG_FILE_STDERR=$PATH_TO_NODE/logs/stderr.log

    # Set node ports.
    PORT_NODE_API_REST=$(get_node_port_rest "$NODE_ID")
    PORT_NODE_API_RPC=$(get_node_port_rpc "$NODE_ID")
    PORT_NODE_API_EVENT=$(get_node_port_sse "$NODE_ID")

    # Set validator network addresses.
    NETWORK_BIND_ADDRESS=$(get_network_bind_address "$NODE_ID")
    NETWORK_KNOWN_ADDRESSES=[$(get_network_known_addresses "$NODE_ID")]

    # Set daemon process unit.
    NODE_PROCESS_UNIT=$(get_process_name_of_node "$NODE_ID")

    # Set trusted hash arg.
    if [ -n "$TRUSTED_HASH" ]; then
        TRUSTED_HASH_ARG=--config-ext=node.trusted_hash="${TRUSTED_HASH}"
    else
        TRUSTED_HASH_ARG=
    fi

    # Run node via systemd.
    systemd-run \
        --user \
        --unit "$NODE_PROCESS_UNIT" \
        --description "Casper Dev Net ${NET_ID} Node ${NODE_ID}" \
        --collect \
        --no-block \
        --property=Type=notify \
        --property=TimeoutSec=600 \
        --property=WorkingDirectory="${NCTL_CASPER_HOME}" \
        "$DEPS" \
        --setenv=RUST_LOG=casper=trace \
        --property=StandardOutput=file:"$PATH_TO_NODE_LOG_FILE_STDOUT" \
        --property=StandardError=file:"$PATH_TO_NODE_LOG_FILE_STDERR" \
        -- \
        cargo run -p casper-node -- \
            validator \
            "$PATH_TO_NODE_CONFIG" \
            --config-ext=network.systemd_support=true \
            --config-ext=consensus.secret_key_path="$PATH_TO_NODE_SECRET_KEY" \
            --config-ext event_stream_server.address=0.0.0.0:"$PORT_NODE_API_EVENT" \
            --config-ext logging.format="json" \
            --config-ext network.bind_address="$NETWORK_BIND_ADDRESS" \
            --config-ext network.known_addresses="$NETWORK_KNOWN_ADDRESSES" \
            --config-ext node.chainspec_config_path="$PATH_TO_NET_CHAINSPEC" \
            --config-ext rest_server.address=0.0.0.0:"$PORT_NODE_API_REST" \
            --config-ext rpc_server.address=0.0.0.0:"$PORT_NODE_API_RPC" \
            --config-ext=storage.path="$PATH_TO_NODE_STORAGE" \
            --config-ext=network.gossip_interval=1000 \
            "$TRUSTED_HASH_ARG"
}

#######################################
# Spins up all nodes using systemd.
#######################################
function do_node_start_all()
{        
    # Step 1: start bootstraps.
    log "... starting genesis bootstraps"
    do_node_start_group "$NCTL_PROCESS_GROUP_1"
    sleep 1.0

    # Step 2: start non-bootstraps.
    log "... starting genesis non-bootstraps"
    do_node_start_group "$NCTL_PROCESS_GROUP_2"
}

#######################################
# Spins up a group of nodes.
# Arguments:
#   Node group identifier.
#######################################
function do_node_start_group()
{
    local GROUP_ID=${1}

    echo "TODO: implement do_node_start_group $GROUP_ID"
}

#######################################
# Renders to stdout status of a node running under systemd.
# Arguments:
#   Node ordinal identifier.
#######################################
function do_node_status()
{
    local NODE_ID=${1}
    local NODE_PROCESS_NAME
    
    NODE_PROCESS_NAME=$(get_process_name_of_node_in_group "$NODE_ID")

    systemctl --user status "$NODE_PROCESS_NAME"
}

#######################################
# Renders to stdout status of all nodes running under supervisord.
# Arguments:
#   Network ordinal identifier.
#######################################
function do_node_status_all()
{
    echo "TODO: implement"
}

#######################################
# Stops a node running via systemd.
# Arguments:
#   Node ordinal identifier.
#######################################
function do_node_stop()
{
    local NODE_ID=${1}
    local NODE_PROCESS_NAME
    
    NODE_PROCESS_NAME=$(get_process_name_of_node_in_group "$NODE_ID")
    
    systemctl --user stop "$NODE_PROCESS_NAME"
}

#######################################
# Stops all nodes running via supervisord.
#######################################
function do_node_stop_all()
{
    echo "TODO: implement"
}
