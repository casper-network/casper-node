#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

#######################################
# Destructure input args.
#######################################

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        *)
    esac
done

export NET_ID=${NET_ID:-1}

#######################################
# Main
#######################################

log "net-$NET_ID: asset tear-down begins ... please wait"

log "... stopping nodes"
source "$NCTL"/sh/node/stop.sh node=all

if [ "$NCTL_DAEMON_TYPE" = "supervisord" ]; then
    log "... stopping supervisord"
    do_supervisord_kill
fi

log "... deleting files"
rm -rf "$(get_path_to_net)"
rm -rf "$NCTL"/dumps

log "net-$NET_ID: asset tear-down complete"
