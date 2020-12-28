#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/contracts-transfers/utils.sh

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset BATCH_ID
unset INTERVAL
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        batch) BATCH_ID=${VALUE} ;;
        interval) INTERVAL=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

do_dispatch_batch "${BATCH_ID:-1}" \
                  "transfer-wasm" \
                  "${INTERVAL:-0.01}" \
                  "${NODE_ID:-"random"}"
