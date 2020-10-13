#!/usr/bin/env bash
#
#######################################
# Toggles a network node.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#######################################

# Import utils.
source $NCTL/sh/daemon/supervisord/utils.sh

# Derive current process status by inspecting supervisorctl status output.
node_status=$(supervisorctl -c "$(get_path_net_supervisord_cfg $1)" status "$(get_node_process_name $1 $2)")

# Map status to action & execute.
case "$node_status" in 
  *EXITED*)
    source $NCTL/sh/daemon/supervisord/node_start.sh $1 $2
    ;;
  *FATAL*)
    source $NCTL/sh/daemon/supervisord/node_start.sh $1 $2
    ;;
  *RUNNING*)
    source $NCTL/sh/daemon/supervisord/node_stop.sh $1 $2
    ;;
  *STOPPED*)
    source $NCTL/sh/daemon/supervisord/node_start.sh $1 $2
    ;;
esac
