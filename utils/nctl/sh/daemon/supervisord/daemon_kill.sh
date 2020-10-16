#!/usr/bin/env bash
#
#######################################
# Kills supervisord (if necessary).
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#######################################

# Import utils.
source $NCTL/sh/daemon/supervisord/utils.sh

# If sock file exists then stop daemon.
if [ -e "$(get_path_net_supervisord_sock $1)" ]; then
    supervisorctl -c "$(get_path_net_supervisord_cfg $1)" stop all &>/dev/null
    supervisorctl -c "$(get_path_net_supervisord_cfg $1)" shutdown &>/dev/null
fi
