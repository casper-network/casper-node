#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

unset OFFSET
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
        offset) OFFSET=${VALUE} ;;
        *)
    esac
done

OFFSET=${OFFSET:-1}
NODE_ID=${NODE_ID:-6}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Await until N blocks have been added to linear block chain.
await_n_blocks "$OFFSET" true

# Start node.
source "$NCTL"/sh/node/start.sh \
    hash="$TRUSTED_HASH" \
    node="$NODE_ID" \
    loglevel="$LOG_LEVEL" 
