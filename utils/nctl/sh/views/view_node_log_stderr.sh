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

less "$(get_path_to_node "${NODE_ID:-1}")"/logs/stderr.log
