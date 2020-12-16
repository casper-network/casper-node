#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $NCTL/sh/views/funcs.sh

unset NET_ID
unset NODE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        node) NODE_ID=${VALUE} ;;
        *)
    esac
done

less $(get_path_to_node ${NET_ID:-1} ${NODE_ID:-1})/config/node-config.toml
