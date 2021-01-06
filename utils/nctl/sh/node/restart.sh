#!/usr/bin/env bash

unset CLEAN
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
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

# Clear storage.
if [ "$CLEAN" = true ]; then
    log "node-$NODE_ID: clearing storage"
    rm "$(get_path_to_node_storage "$NODE_ID")/*"
fi

# Start.
source "$NCTL"/sh/node/start.sh node="$NODE_ID"
