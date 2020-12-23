#!/usr/bin/env bash

unset OFFSET
unset LOG_LEVEL
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in        
        offset) OFFSET=${VALUE} ;;
        loglevel) LOG_LEVEL=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

OFFSET=${OFFSET:-1}
NODE_ID=${NODE_ID:-6}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/utils.sh

await_n_eras \
    $OFFSET \
    true

source $NCTL/sh/node/start.sh \
    node=$NODE_ID \
    loglevel=$LOG_LEVEL
