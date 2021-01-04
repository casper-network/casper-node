#!/usr/bin/env bash

unset LOG_LEVEL
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
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

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh


if [ "$NODE_ID" == "all" ]; then
    log "net-$NET_ID: starting node(s) begins ... please wait"
    do_node_start_all
    log "net-$NET_ID: starting node(s) complete"
else
    log "starting node :: node-$NODE_ID"
    do_node_start "$NODE_ID"
fi

sleep 1.0
source "$NCTL"/sh/node/status.sh node="$NODE_ID"

