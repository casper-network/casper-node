#!/usr/bin/env bash
#
#######################################
# Sets artefacts pertaining to network daemon.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Path to network directory.
#   Network ordinal identifier.
#   Nodeset count.
#   Boostrap count.
#######################################

# Set supervisord.conf file.
touch  $1/daemon/config/supervisord.conf

# Set supervisord.conf header.
cat >> $1/daemon/config/supervisord.conf <<- EOM
[unix_http_server]
file=$1/daemon/socket/supervisord.sock ;

[supervisord]
logfile=$1/daemon/logs/supervisord.log ;
logfile_maxbytes=50MB ;
logfile_backups=10 ;
loglevel=info ;
pidfile=$1/daemon/socket/supervisord.pid ;

[rpcinterface:supervisor]
supervisor.rpcinterface_factory = supervisor.rpcinterface:make_main_rpcinterface

[supervisorctl]
serverurl=unix:///$1/daemon/socket/supervisord.sock ;
EOM

# Set supervisord.conf app sections.
for node_id in $(seq 1 $3)
do
    # Set paths.
    PATH_NET=$1
    PATH_NET_CHAINSPEC=$PATH_NET/chainspec/chainspec.toml
    PATH_NODE=$PATH_NET/nodes/node-$node_id
    PATH_NODE_CONFIG=$PATH_NODE/config/node-config.toml
    PATH_NODE_STORAGE=$PATH_NODE/storage
    PATH_NODE_SECRET_KEY=$PATH_NODE/keys/secret_key.pem

    # Set ports.
    NODE_API_PORT_REST=$(get_node_port_rest $2 $node_id)
    NODE_API_PORT_RPC=$(get_node_port_rpc $2 $node_id)
    NODE_API_PORT_SSE=$(get_node_port_sse $2 $node_id)

    # Set validator network addresses.
    NETWORK_BIND_ADDRESS=$(get_network_bind_address $2 $node_id $4)
    NETWORK_KNOWN_ADDRESSES=$(get_network_known_addresses $2 $node_id $4)

    # Add supervisord application section.
    cat >> $1/daemon/config/supervisord.conf <<- EOM

[program:casper-net-$2-node-$node_id]
autostart=false
autorestart=false
command=$1/bin/casper-node validator $PATH_NODE_CONFIG \
    --config-ext consensus.secret_key_path=$PATH_NODE_SECRET_KEY \
    --config-ext event_stream_server.address=0.0.0.0:$NODE_API_PORT_SSE \
    --config-ext logging.format="json" \
    --config-ext network.bind_address=$NETWORK_BIND_ADDRESS \
    --config-ext network.known_addresses=[$NETWORK_KNOWN_ADDRESSES] \
    --config-ext node.chainspec_config_path=$PATH_NET_CHAINSPEC \
    --config-ext rest_server.address=0.0.0.0:$NODE_API_PORT_REST \
    --config-ext rpc_server.address=0.0.0.0:$NODE_API_PORT_RPC \
    --config-ext storage.path=$PATH_NODE_STORAGE ;
numprocs=1
numprocs_start=0
stderr_logfile=$PATH_NODE/logs/stderr.log ;
stderr_logfile_backups=5 ;
stderr_logfile_maxbytes=50MB ;
stdout_logfile=$PATH_NODE/logs/stdout.log ;
stdout_logfile_backups=5 ;
stdout_logfile_maxbytes=50MB ;
EOM
done
