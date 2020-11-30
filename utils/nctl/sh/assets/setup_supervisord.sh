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
    path_net=$1
    path_net_chainspec=$path_net/chainspec/chainspec.toml
    path_node=$path_net/nodes/node-$node_id
    path_node_storage=$path_node/storage
    path_node_secret_key=$path_node/keys/secret_key.pem

    # Set ports.
    port_node_api_rpc=$(get_node_port $NCTL_BASE_PORT_RPC $2 $node_id)
    port_node_api_rest=$(get_node_port $NCTL_BASE_PORT_REST $2 $node_id)
    port_node_api_event=$(get_node_port $NCTL_BASE_PORT_EVENT $2 $node_id)

    # Set validator network addresses.
    network_bind_address=$(get_network_bind_address $2 $node_id $4)
    network_known_addresses=$(get_network_known_addresses $2 $node_id $4)

    # Add supervisord application section.
    cat >> $1/daemon/config/supervisord.conf <<- EOM

[program:casper-net-$2-node-$node_id]
autostart=false
autorestart=false
command=$1/bin/casper-node validator $path_node/config/node-config.toml \
    --config-ext consensus.secret_key_path=$path_node_secret_key \
    --config-ext event_stream_server.address=0.0.0.0:$port_node_api_event \
    --config-ext logging.format="json" \
    --config-ext network.bind_address=$network_bind_address \
    --config-ext network.known_addresses=[$network_known_addresses] \
    --config-ext node.chainspec_config_path=$path_net_chainspec \
    --config-ext rest_server.address=0.0.0.0:$port_node_api_rest \
    --config-ext rpc_server.address=0.0.0.0:$port_node_api_rpc \
    --config-ext storage.path=$path_node_storage ;
numprocs=1
numprocs_start=0
stderr_logfile=$path_node/logs/stderr.log ;
stderr_logfile_backups=5 ;
stderr_logfile_maxbytes=50MB ;
stdout_logfile=$path_node/logs/stdout.log ;
stdout_logfile_backups=5 ;
stdout_logfile_maxbytes=50MB ;
EOM
done
