#!/usr/bin/env bash
#
#######################################
# Starts a network node.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifer.
#   Node ordinal identifer.
#######################################

# Import utils.
source $NCTL/sh/daemon/supervisord/utils.sh

# Ensure daemon is up.
source $NCTL/sh/daemon/supervisord/daemon_start.sh $1 

# Start nodeset.
if [ $2 = "all" ]; then
    supervisorctl -c "$(get_path_net_supervisord_cfg $1)" start all  > /dev/null 2>&1
else
    supervisorctl -c "$(get_path_net_supervisord_cfg $1)" start "$(get_node_process_name $1 $2)"  > /dev/null 2>&1
fi

