#!/usr/bin/env bash

unset NET_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in
        net) NET_ID=${VALUE} ;;
        *)
    esac
done

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/utils.sh

less $(get_path_to_net ${NET_ID:-1})/chainspec/accounts.csv
