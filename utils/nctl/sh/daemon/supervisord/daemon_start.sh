#!/usr/bin/env bash
#
#######################################
# Starts supervisord (if necessary).
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#######################################

# Import utils.
source $NCTL/sh/daemon/supervisord/utils.sh

# If sock file not found then start daemon.
if [ ! -e "$(get_path_net_supervisord_sock $1)" ]; then
    supervisord -c "$(get_path_net_supervisord_cfg $1)"
    sleep 3.0
fi
