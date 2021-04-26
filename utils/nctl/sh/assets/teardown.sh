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

log "asset tear-down begins ... please wait"

# Stop network (if running).
source "$NCTL"/sh/node/stop.sh node=all

# Delete assets.
if [ -d "$(get_path_to_net)" ]; then
    log "... deleting files"
    rm -rf "$(get_path_to_net)"
    rm -rf "$NCTL"/dumps
fi

# Allow machine to chill.
sleep 2.0

log "asset tear-down complete"
