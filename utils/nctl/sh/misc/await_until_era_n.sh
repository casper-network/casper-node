#!/usr/bin/env bash

unset FUTURE_ERA_ID
unset LOG_OUTPUT
unset SLEEP_INTERVAL

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in        
        era) FUTURE_ERA_ID=${VALUE} ;;
        log) LOG_OUTPUT=${VALUE} ;;
        sleep_interval) SLEEP_INTERVAL=${VALUE} ;;
        *)
    esac
done

FUTURE_ERA_ID=${FUTURE_ERA_ID:-1}
LOG_OUTPUT=${LOG_OUTPUT:-'false'}
SLEEP_INTERVAL=${SLEEP_INTERVAL:-'5'}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

source "$NCTL"/sh/utils/main.sh

while [ "$(get_chain_era)" -lt "$FUTURE_ERA_ID" ];
do
    if [ "$LOG_OUTPUT" == 'true' ]; then
        log "current era = $(get_chain_era) :: future era = $FUTURE_ERA_ID :: sleeping $SLEEP_INTERVAL seconds"
    fi
    sleep "$SLEEP_INTERVAL"
done
