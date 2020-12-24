#!/usr/bin/env bash

unset FUTURE_ERA_ID

for ARGUMENT in "$@"
do
    KEY=$(echo $ARGUMENT | cut -f1 -d=)
    VALUE=$(echo $ARGUMENT | cut -f2 -d=)
    case "$KEY" in        
        era) FUTURE_ERA_ID=${VALUE} ;;
        *)
    esac
done

FUTURE_ERA_ID=${FUTURE_ERA_ID:-1}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source $NCTL/sh/utils/main.sh

while [ $(get_chain_era) -lt $FUTURE_ERA_ID ];
do
    sleep 5.0
done
