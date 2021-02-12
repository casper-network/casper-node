#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

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

if [ "$NODE_ID" = "all" ]; then
    rm "$(get_path_to_net)"/nodes/node-*/logs/*.log > /dev/null 2>&1
else
    rm "$(get_path_to_node_logs "$NODE_ID")"/*.log > /dev/null 2>&1
fi
