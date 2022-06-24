#!/usr/bin/env bash

#######################################
# Awaits for the chain to proceed N eras.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Future era offset to apply.
#######################################
function await_n_eras()
{
    local OFFSET=${1:-'1'}
    local EMIT_LOG=${2:-'false'}
    local SLEEP_INTERVAL=${3:-'20.0'}
    local NODE_ID=${4:-''}
    local TIME_OUT=${5:-''}
    local CURRENT
    local FUTURE
    local LOG_OUTPUT

    CURRENT=$(get_chain_era "$NODE_ID")
    FUTURE=$((CURRENT + OFFSET))

    while [ "$CURRENT" -lt "$FUTURE" ];
    do

        LOG_OUTPUT="current era = $CURRENT :: future era = $FUTURE :: sleeping $SLEEP_INTERVAL seconds"

        if [ "$EMIT_LOG" = true ] && [ ! -z "$TIME_OUT" ]; then
            log "$LOG_OUTPUT :: timeout = $TIME_OUT seconds"
        elif [ "$EMIT_LOG" = true ]; then
            log "$LOG_OUTPUT"
        fi

        sleep "$SLEEP_INTERVAL"

        if [ ! -z "$TIME_OUT" ]; then
            # Using jq since its required by NCTL anyway to do this floating point arith
            # ... done to maintain backwards compatibility
            TIME_OUT=$(jq -n "$TIME_OUT-$SLEEP_INTERVAL")

            if [ "$TIME_OUT" -le "0" ]; then
                log "ERROR: Timed out before reaching future era = $FUTURE"
                # https://stackoverflow.com/a/54344104
                return 1 2>/dev/null
            fi
        fi

        CURRENT=$(get_chain_era "$NODE_ID")
    done

    if [ "$EMIT_LOG" = true ]; then
        log "current era = $CURRENT"
    fi
}

#######################################
# Awaits for the chain to proceed N blocks.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Future block height offset to apply.
#######################################
function await_n_blocks()
{
    local OFFSET=${1}
    local EMIT_LOG=${2:-false}
    local NODE_ID=${3:-''}

    local CURRENT
    local FUTURE

    CURRENT=$(get_chain_height "$NODE_ID")
    FUTURE=$((CURRENT + OFFSET))

    while [ "$CURRENT" -lt "$FUTURE" ];
    do
        if [ "$EMIT_LOG" = true ]; then
            log "current block height = $CURRENT :: future height = $FUTURE ... sleeping 2 seconds"
        fi
        sleep 2.0
        CURRENT=$(get_chain_height "$NODE_ID")
    done

    if [ "$EMIT_LOG" = true ]; then
        log "current block height = $CURRENT"
    fi
}

#######################################
# Awaits for the chain to proceed N eras.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Future era offset to apply.
#######################################
function await_until_era_n()
{
    local ERA=${1}
    local EMIT_LOG=${2:-false}

    while [ "$(get_chain_era)" -lt "$ERA" ];
    do
        if [ "$EMIT_LOG" = true ]; then
            log "waiting for future era = $ERA ... sleeping 2 seconds"
        fi
        sleep 2.0
    done
}

#######################################
# Awaits for the chain to proceed N blocks.
# Arguments:
#   Network ordinal identifier.
#   Node ordinal identifier.
#   Future block offset to apply.
#######################################
function await_until_block_n()
{
    local HEIGHT=${1}

    while [ "$HEIGHT" -lt "$(get_chain_height)" ];
    do
        sleep 10.0
    done
}
