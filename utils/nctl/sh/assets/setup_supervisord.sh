#!/usr/bin/env bash
#
#######################################
# Sets artefacts pertaining to network daemon.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Path to network directory.
#   Network ordinal identifer.
#   Nodeset count.
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
    cat >> $1/daemon/config/supervisord.conf <<- EOM

[program:casper-net-$2-node-$node_id]
autostart=false
autorestart=false
command=$1/bin/casper-node validator $1/nodes/node-$node_id/config/node-config.toml ;
numprocs=1
numprocs_start=0
stderr_logfile=$1/nodes/node-$node_id/logs/stderr.log ;
stderr_logfile_backups=5 ;
stderr_logfile_maxbytes=50MB ;
stdout_logfile=$1/nodes/node-$node_id/logs/stdout.log ;
stdout_logfile_backups=5 ;
stdout_logfile_maxbytes=50MB ;
EOM
done
