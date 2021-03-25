#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh

# ----------------------------------------------------------------
# ARGS
# ----------------------------------------------------------------

unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

NODE_ID=${NODE_ID:-"all"}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

if [ "$NODE_ID" == "all" ]; then
    do_node_stop_all
    do_node_status_all
else
    log "node-$NODE_ID: stopping node ... "
    do_node_stop "$NODE_ID"
    sleep 1.0
    source "$NCTL"/sh/node/status.sh node="$NODE_ID"
fi
