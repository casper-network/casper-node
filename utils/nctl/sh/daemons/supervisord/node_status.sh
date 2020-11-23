#!/usr/bin/env bash
#
#######################################
# Displays node(s) status.
# Globals:
#   NCTL - path to nctl home directory.
# Arguments:
#   Network ordinal identifier.
#######################################

# Import utils.
source $NCTL/sh/daemons/supervisord/utils.sh

# Ensure daemon is up.
source $NCTL/sh/daemons/supervisord/daemon_start.sh $1

# Display nodeset state.
log "supervisord node process states:"
supervisorctl -c "$(get_path_net_supervisord_cfg $1)" status all
