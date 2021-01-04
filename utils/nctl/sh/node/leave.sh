#!/usr/bin/env bash

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

NODE_ID=${NODE_ID:-6}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

