#!/usr/bin/env bash

source $NCTL/sh/utils.sh
source $NCTL/sh/views/funcs.sh

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

render_account_hash \
    ${NET_ID:-1} \
    $NCTL_ACCOUNT_TYPE_FAUCET
