#!/usr/bin/env bash

unset OFFSET

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in        
        offset) OFFSET=${VALUE} ;;
        *)
    esac
done

OFFSET=${OFFSET:-1}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source "$NCTL"/sh/utils/main.sh

await_n_blocks "$OFFSET" true
