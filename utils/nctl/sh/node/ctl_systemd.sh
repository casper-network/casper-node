#!/usr/bin/env bash
#
# Set of supervisord node process control functions.
# Globals:
#   NCTL - path to nctl home directory.

# Import utils.
source $NCTL/sh/utils.sh

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
    local NET_ID=${1}
    local NODE_ID=${2}
    local BOOTSTRAP_COUNT=${3}
    local TRUSTED_HASH=${4:-}
    
    # Set net asset paths.
    local PATH_TO_NET=$(get_path_to_net $NET_ID)
    local PATH_TO_NET_CHAINSPEC=$PATH_TO_NET/chainspec/chainspec.toml

    # Set node asset paths.
    local PATH_TO_NODE=$(get_path_to_node $NET_ID $NODE_ID)
    local PATH_TO_NODE_CONFIG=$PATH_TO_NODE/config/node-config.toml
    local PATH_TO_NODE_SECRET_KEY=$PATH_TO_NODE/keys/secret_key.pem
    local PATH_TO_NODE_STORAGE=$PATH_TO_NODE/storage
    local PATH_TO_NODE_LOG_FILE_STDOUT=$PATH_TO_NODE/logs/net-${NET_ID}-node-${NODE_ID}-stdout.log
    local PATH_TO_NODE_LOG_FILE_STDERR=$PATH_TO_NODE/logs/net-${NET_ID}-node-${NODE_ID}-stderr.log

    # Set node ports.
    local PORT_NODE_API_REST=$(get_node_port_rest $NET_ID $NODE_ID)
    local PORT_NODE_API_RPC=$(get_node_port_rpc $NET_ID $NODE_ID)
    local PORT_NODE_API_EVENT=$(get_node_port_sse $NET_ID $NODE_ID)

    # Set validator network addresses.
    local NETWORK_BIND_ADDRESS=$(get_network_bind_address $NET_ID $NODE_ID $BOOTSTRAP_COUNT)
    local NETWORK_KNOWN_ADDRESSES=$(get_network_known_addresses $NET_ID $NODE_ID $BOOTSTRAP_COUNT)

    # Set daemon process unit.
    local NODE_PROCESS_UNIT=$(get_process_name_of_node $NET_ID $NODE_ID)

    # Set trusted hash arg.
    if [ ! -z "$TRUSTED_HASH" ]; then
        TRUSTED_HASH_ARG=--config-ext=node.trusted_hash="${TRUSTED_HASH}"
    else
        TRUSTED_HASH_ARG=
    fi

    # Run node via systemd.
    systemd-run \
        --user \
        --unit $NODE_PROCESS_UNIT \
        --description "Casper Dev Net ${NET_ID} Node ${NODE_ID}" \
        --collect \
        --no-block \
        --property=Type=notify \
        --property=TimeoutSec=600 \
        --property=WorkingDirectory=${NCTL_CASPER_HOME} \
        $DEPS \
        --setenv=RUST_LOG=casper=trace \
        --property=StandardOutput=file:${PATH_TO_NODE_LOG_FILE_STDOUT} \
        --property=StandardError=file:${PATH_TO_NODE_LOG_FILE_STDERR} \
        -- \
        cargo run -p casper-node -- \
            validator \
            $PATH_TO_NODE_CONFIG \
            --config-ext=network.systemd_support=true \
            --config-ext=consensus.secret_key_path=${PATH_TO_NODE_SECRET_KEY} \
            --config-ext event_stream_server.address=0.0.0.0:$PORT_NODE_API_EVENT \
            --config-ext logging.format="json" \
            --config-ext network.bind_address=$NETWORK_BIND_ADDRESS \
            --config-ext network.known_addresses=[$NETWORK_KNOWN_ADDRESSES] \
            --config-ext node.chainspec_config_path=$PATH_TO_NET_CHAINSPEC \
            --config-ext rest_server.address=0.0.0.0:$PORT_NODE_API_REST \
            --config-ext rpc_server.address=0.0.0.0:$PORT_NODE_API_RPC \
            --config-ext=storage.path=${PATH_TO_NODE_STORAGE} \
            --config-ext=network.gossip_interval=1000 \
            ${TRUSTED_HASH_ARG}
}

#######################################
# Spins up all nodes using systemd.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
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
    for IDX in $(seq 1 $NET_NODE_COUNT)
    do
        if [ $IDX -gt $NET_BOOTSTRAP_COUNT ]; then
            log "net-$NET_ID: starting node $IDX"
            do_node_start $NET_ID $IDX
        fi
    done
}

#######################################
# Renders to stdout status of a node running under systemd.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function do_node_status()
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local NODE_PROCESS_UNIT=$(get_process_name_of_node $NET_ID $NODE_ID)

    systemctl --user status  $NODE_PROCESS_UNIT
}

#######################################
# Renders to stdout status of all nodes running under supervisord.
# Arguments:
#   Network ordinal identifier.
#######################################
function do_node_status_all()
{
    echo "TODO: emit process status of all nodes"
}

#######################################
# Stops a node running via systemd.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################
function do_node_stop()
{
    local NET_ID=${1}
    local NODE_ID=${2}
    local NODE_PROCESS_UNIT=$(get_process_name_of_node $NET_ID $NODE_ID)
    
    systemctl --user stop $NODE_PROCESS_UNIT
}
