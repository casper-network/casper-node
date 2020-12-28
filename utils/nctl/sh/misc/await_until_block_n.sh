#!/usr/bin/env bash

unset FUTURE_HEIGHT

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in        
        height) FUTURE_HEIGHT=${VALUE} ;;
        *)
    esac
done

FUTURE_HEIGHT=${FUTURE_HEIGHT:-1}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source "$NCTL"/sh/utils/main.sh

while [ "$(get_chain_height)" -lt "$FUTURE_HEIGHT" ];
do
    sleep 1.0
done
