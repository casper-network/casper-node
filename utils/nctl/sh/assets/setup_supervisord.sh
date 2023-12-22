#!/usr/bin/env bash
#
#######################################
# Sets artefacts pertaining to network daemon.
# Globals:
#   NCTL_PROCESS_GROUP_1 - process 1 group identifier.
#   NCTL_PROCESS_GROUP_2 - process 2 group identifier.
#   NCTL_PROCESS_GROUP_3 - process 3 group identifier.
#######################################

PATH_TO_NET=$(get_path_to_net)
PATH_SUPERVISOR_CONFIG=$(get_path_net_supervisord_cfg)
touch  "$PATH_SUPERVISOR_CONFIG"

# ------------------------------------------------------------------------
# Set supervisord.conf header.
# ------------------------------------------------------------------------
cat >> "$PATH_SUPERVISOR_CONFIG" <<- EOM
[unix_http_server]
file=$PATH_TO_NET/daemon/socket/supervisord.sock ;

[supervisord]
logfile=$PATH_TO_NET/daemon/logs/supervisord.log ;
logfile_maxbytes=200MB ;
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
for NODE_ID in $(seq 1 "$(get_count_of_nodes)")
do
    PATH_NODE=$(get_path_to_node "$NODE_ID")
    PATH_NODE_BIN=$(get_path_to_node_bin "$NODE_ID")
    PATH_NODE_CONFIG=$(get_path_to_node_config "$NODE_ID")
    PATH_NODE_LOGS=$(get_path_to_node_logs "$NODE_ID")

    local NODE_PROTOCOL_VERSION

    NODE_PROTOCOL_VERSION=$(get_node_protocol_version_from_fs "$NODE_ID" "_")

    cat >> "$PATH_SUPERVISOR_CONFIG" <<- EOM

[program:casper-net-$NET_ID-node-$NODE_ID]
autostart=false
autorestart=false
command=$PATH_NODE_BIN/casper-node-launcher
environment=CASPER_BIN_DIR="$PATH_NODE_BIN",CASPER_CONFIG_DIR="$PATH_NODE_CONFIG"
numprocs=1
numprocs_start=0
startsecs=0
stopsignal=TERM
stopwaitsecs=5
stopasgroup=true
stderr_logfile=$PATH_NODE_LOGS/stderr.log ;
stderr_logfile_backups=5 ;
stderr_logfile_maxbytes=500MB ;
stdout_logfile=$PATH_NODE_LOGS/stdout.log ;
stdout_logfile_backups=5 ;
stdout_logfile_maxbytes=500MB ;

[program:casper-net-$NET_ID-sidecar-$NODE_ID]
autostart=false
autorestart=false
command=$NCTL/sh/rpc-sidecar/start-latest.sh node_dir=$PATH_NODE
environment=NODE_DIR="$PATH_NODE"
numprocs=1
numprocs_start=0
startsecs=0
stopsignal=TERM
stopwaitsecs=5
stopasgroup=true
stderr_logfile=$PATH_NODE_LOGS/sidecar-stderr.log ;
stderr_logfile_backups=5 ;
stderr_logfile_maxbytes=500MB ;
stdout_logfile=$PATH_NODE_LOGS/sidecar-stdout.log ;
stdout_logfile_backups=5 ;
stdout_logfile_maxbytes=500MB ;
EOM
done

# ------------------------------------------------------------------------
# Set supervisord.conf group sections.
# ------------------------------------------------------------------------
cat >> "$PATH_SUPERVISOR_CONFIG" <<- EOM

[group:$NCTL_PROCESS_GROUP_1]
programs=$(get_process_group_members "$NCTL_PROCESS_GROUP_1")

[group:$NCTL_PROCESS_GROUP_2]
programs=$(get_process_group_members "$NCTL_PROCESS_GROUP_2")

[group:$NCTL_PROCESS_GROUP_3]
programs=$(get_process_group_members "$NCTL_PROCESS_GROUP_3")

EOM
