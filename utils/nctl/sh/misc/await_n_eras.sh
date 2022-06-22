#!/usr/bin/env bash

unset OFFSET
unset EMIT_LOG
unset SLEEP_INTERVAL
unset NODE_ID
unset TIME_OUT

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in        
        offset) OFFSET=${VALUE} ;;
        emit_log) EMIT_LOG=${VALUE} ;;
        sleep_interval) SLEEP_INTERVAL=${VALUE} ;;
        node_id) NODE_ID=${VALUE} ;;
        timeout) TIME_OUT=${VALUE} ;;
        *)
    esac
done

OFFSET=${OFFSET:-'1'}
EMIT_LOG=${EMIT_LOG:-'true'}
SLEEP_INTERVAL=${SLEEP_INTERVAL:-'20.0'}
NODE_ID=${NODE_ID:-''}
TIME_OUT=${TIME_OUT:-''}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source "$NCTL"/sh/utils/main.sh

await_n_eras "$OFFSET" "$EMIT_LOG" "$SLEEP_INTERVAL" "$NODE_ID" "$TIME_OUT"
