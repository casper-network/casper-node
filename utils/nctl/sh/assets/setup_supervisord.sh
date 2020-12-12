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

PATH_TO_NET=$1
NET_ID=$2
COUNT_NODES=$3
COUNT_BOOTSTRAPS=$4

# Set supervisord.conf file.
touch  $PATH_TO_NET/daemon/config/supervisord.conf

# ------------------------------------------------------------------------
# Set supervisord.conf header.
# ------------------------------------------------------------------------
cat >> $PATH_TO_NET/daemon/config/supervisord.conf <<- EOM
[unix_http_server]
file=$PATH_TO_NET/daemon/socket/supervisord.sock ;

[supervisord]
logfile=$PATH_TO_NET/daemon/logs/supervisord.log ;
logfile_maxbytes=50MB ;
logfile_backups=10 ;
loglevel=info ;
pidfile=$PATH_TO_NET/daemon/socket/supervisord.pid ;

[rpcinterface:supervisor]
supervisor.rpcinterface_factory = supervisor.rpcinterface:make_main_rpcinterface

[supervisorctl]
serverurl=unix:///$PATH_TO_NET/daemon/socket/supervisord.sock ;
EOM

# ------------------------------------------------------------------------
# Set supervisord.conf app sections.
# ------------------------------------------------------------------------
for IDX in $(seq 1 $(($COUNT_NODES * 2)))
do
    # Set paths.
    PATH_NET=$PATH_TO_NET
    PATH_NET_CHAINSPEC=$PATH_NET/chainspec/chainspec.toml
    PATH_NODE=$PATH_NET/nodes/node-$IDX
    PATH_NODE_CONFIG=$PATH_NODE/config/node-config.toml
    PATH_NODE_STORAGE=$PATH_NODE/storage
    PATH_NODE_SECRET_KEY=$PATH_NODE/keys/secret_key.pem

    # Set ports.
    NODE_API_PORT_REST=$(get_node_port_rest $NET_ID $IDX)
    NODE_API_PORT_RPC=$(get_node_port_rpc $NET_ID $IDX)
    NODE_API_PORT_SSE=$(get_node_port_sse $NET_ID $IDX)

    # Set validator network addresses.
    NETWORK_BIND_ADDRESS=$(get_network_bind_address $NET_ID $IDX $COUNT_BOOTSTRAPS)
    NETWORK_KNOWN_ADDRESSES=$(get_network_known_addresses $NET_ID $COUNT_BOOTSTRAPS)

    # Add supervisord application section.
    cat >> $PATH_TO_NET/daemon/config/supervisord.conf <<- EOM

[program:casper-net-$NET_ID-node-$IDX]
autostart=false
autorestart=false
command=$PATH_TO_NET/bin/casper-node validator $PATH_NODE_CONFIG \
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

# ------------------------------------------------------------------------
# Set supervisord.conf group sections.
# ------------------------------------------------------------------------
cat >> $PATH_TO_NET/daemon/config/supervisord.conf <<- EOM

[group:$NCTL_PROCESS_GROUP_1]
programs=$(get_process_group_members $NET_ID $NCTL_PROCESS_GROUP_1)

[group:$NCTL_PROCESS_GROUP_2]
programs=$(get_process_group_members $NET_ID $NCTL_PROCESS_GROUP_2)

[group:$NCTL_PROCESS_GROUP_3]
programs=$(get_process_group_members $NET_ID $NCTL_PROCESS_GROUP_3)

EOM
