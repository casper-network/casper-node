#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

unset _NODE_ID
unset _PATH_TO_NODE
unset _ACTIVE_PROTOCOL_VERSION

for ARGUMENT in "$@"
do
    _KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    _VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$_KEY" in
        node) _NODE_ID=${_VALUE} ;;
        *)
    esac
done

_NODE_ID=${_NODE_ID:-1}

less "$(get_path_to_node_config_file "$_NODE_ID")"
