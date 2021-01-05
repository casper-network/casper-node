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

echo $(get_node_address_event "${NODE_ID:-1}")

curl -N -H "Accept:text/event-stream" "$(get_node_address_event "${NODE_ID:-1}")" | grep "Finality"
