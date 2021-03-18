#!/usr/bin/env bash

unset CLEAN
unset NODE_ID
unset TRUSTED_HASH

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        hash) TRUSTED_HASH=${VALUE} ;;
        clean) CLEAN=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

CLEAN=${CLEAN:-true}
NODE_ID=${NODE_ID:-"all"}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Stop.
if [ "$(get_node_is_up "$NODE_ID")" = true ]; then
    source "$NCTL"/sh/node/stop.sh node="$NODE_ID"
fi

# Clean.
if [ "$CLEAN" = true ]; then
    log "node-$NODE_ID: clearing logs"
    rm "$(get_path_to_node_logs "$NODE_ID")"/*.log > /dev/null 2>&1
    log "node-$NODE_ID: clearing storage"
    rm "$(get_path_to_node_storage "$NODE_ID")"/*.lmdb* > /dev/null 2>&1
    rm "$(get_path_to_node_storage "$NODE_ID")"-consensus/*.dat > /dev/null 2>&1
fi

# Start.
source "$NCTL"/sh/node/start.sh node="$NODE_ID" hash="$TRUSTED_HASH"
