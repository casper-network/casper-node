#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

unset LOG_LEVEL
unset NODE_ID
unset TRUSTED_HASH

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        hash) TRUSTED_HASH=${VALUE} ;;
        loglevel) LOG_LEVEL=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

LOG_LEVEL=${LOG_LEVEL:-$RUST_LOG}
LOG_LEVEL=${LOG_LEVEL:-debug}
export RUST_LOG=$LOG_LEVEL
NODE_ID=${NODE_ID:-"all"}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Start node(s).
if [ "$NODE_ID" == "all" ]; then
    log "starting node(s) begins ... please wait"
    do_node_start_all "$TRUSTED_HASH"
    log "starting node(s) complete"
else
    log "node-$NODE_ID: starting ..."
    do_node_start "$NODE_ID" "$TRUSTED_HASH"
fi

# Display status.
sleep 1.0
source "$NCTL"/sh/node/status.sh node="$NODE_ID"
