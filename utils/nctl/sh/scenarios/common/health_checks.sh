#!/usr/bin/env bash

source "$NCTL"/sh/scenarios/common/itst.sh

# Exit if any of the commands fail.
set -e

function main() {
    log "------------------------------------------------------------"
    log "Starting health checks..."
    log "------------------------------------------------------------"

    log "... Stopping nodes to freeze logs"
    supervisorctl -c "$(get_path_net_supervisord_cfg)" stop all > /dev/null 2>&1

    # Check allowed vs found counts
    assert_error_count
    assert_equivocator_count
    assert_restart_count
    assert_crash_count
    assert_eject_count
    assert_doppel_count

    log "------------------------------------------------------------"
    log "Health checks complete"
    log "------------------------------------------------------------"
}

##########################################################
# Function: assert_error_count
# Uses: Variable ERRORS_ALLOWED
# Description: Searches the log files counting instances
#              of errors. Compares count against
#              the total allowed by the test. If
#              ERRORS_ALLOWED is set to ignore, it will
#              return 0.
#
##########################################################

function assert_error_count() {
    local COUNT
    local TOTAL
    local UNIQ_ERRORS
    local IGNORE_PATTERN
    local IGNORE_ALL
    local ALLOWED_COUNT=0

    log_step "Looking for errors in logs..."

    local ignore_pattern_re='([[:digit:]]+);ignore:(.*)'
    local allowed_count_re='([[:digit:]]+)'

    if [[ $ERRORS_ALLOWED =~ $ignore_pattern_re ]]; then
        ALLOWED_COUNT=${BASH_REMATCH[1]}
        IGNORE_PATTERN=${BASH_REMATCH[2]}
    elif [[ $ERRORS_ALLOWED =~ $allowed_count_re ]]; then
        ALLOWED_COUNT=$ERRORS_ALLOWED
    elif [ "$ERRORS_ALLOWED" = "ignore" ]; then
        IGNORE_ALL=1
    else
        log "Bad error ignore pattern specified"
        exit 1
    fi

    TOTAL='0'
    for i in $(seq 1 "$(get_count_of_nodes)"); do
        if [ -z ${IGNORE_PATTERN+x} ]; then
            COUNT=$(cat "$NCTL"/assets/net-1/nodes/node-"$i"/logs/stdout.log 2>/dev/null | grep -w 'ERROR' | wc -l)
        else
            COUNT=$(cat "$NCTL"/assets/net-1/nodes/node-"$i"/logs/stdout.log 2>/dev/null | grep -w 'ERROR' | grep -v "$IGNORE_PATTERN" | wc -l)
        fi

        TOTAL=$((TOTAL + COUNT))
        log "... node-$i: TOTAL ERROR COUNT = $COUNT"

        UNIQ_ERRORS=$(cat "$NCTL"/assets/net-1/nodes/node-"$i"/logs/stdout.log 2>/dev/null \
                        | { grep -w '"level":"ERROR"' || true; } \
                        | jq .fields.message \
                        | sort \
                        | uniq -c)

        if [ ! -z "$UNIQ_ERRORS" ]; then
            echo "$UNIQ_ERRORS"
        fi

    done

    if [ -n "$IGNORE_ALL" ]; then
        log "Error count ignored by test, skipping $TOTAL errors..."
        return 0
    fi

    if [ "$ALLOWED_COUNT" != "$TOTAL" ]; then
        log "ERROR: ALLOWED: $ALLOWED_COUNT != TOTAL: $TOTAL"
        exit 1
    fi

    log "SUCCESS: ALLOWED: $ALLOWED_COUNT = TOTAL: $TOTAL"
}

##########################################################
# Function: assert_equivocator_count
# Uses: Variable EQUIVS_ALLOWED
# Description: Searches the log files counting instances
#              of equivocators. Compares count against
#              the total allowed by the test. If
#              EQUIVS_ALLOWED is set to ignore, it will
#              return 0.
#
# NOTE: Since equivocations are global we divide the total
#       by the number of nodes it's found on. This is
#       so we don't confuse ourselves when we expect
#       1 but each node has the message and thus the
#       TOTAL would print 5.
##########################################################

function assert_equivocator_count() {
    local COUNT
    local TOTAL
    local GLOBAL_DIVIDER

    log_step "Looking for equivocators in logs..."

    TOTAL='0'
    GLOBAL_DIVIDER='0'
    for i in $(seq 1 "$(get_count_of_nodes)"); do
        COUNT=$(cat "$NCTL"/assets/net-1/nodes/node-"$i"/logs/stdout.log 2>/dev/null | grep -w 'validator equivocated' | wc -l)
        if [ "$COUNT" != "0" ]; then
            GLOBAL_DIVIDER=$((GLOBAL_DIVIDER + 1))
        fi

        TOTAL=$((TOTAL + COUNT))
        log "... node-$i: EQUIV COUNT = $COUNT"
    done

    if [ "$GLOBAL_DIVIDER" != "0" ]; then
        TOTAL=$((TOTAL / GLOBAL_DIVIDER))
    fi

    if [ "$EQUIVS_ALLOWED" = "ignore" ]; then
        log "Equivocator count ignored by test, skipping $TOTAL equivocations..."
        return 0
    fi

    if [ "$EQUIVS_ALLOWED" != "$TOTAL" ]; then
        log "ERROR: ALLOWED: $EQUIVS_ALLOWED != TOTAL: $TOTAL"
        exit 1
    fi

    log "SUCCESS: ALLOWED: $EQUIVS_ALLOWED = TOTAL: $TOTAL"
}

##########################################################
# Function: assert_crash_count
# Uses: Variable CRASH_ALLOWED
# Description: Searches the log files counting instances
#              of crash messages. Compares count against
#              the total allowed by the test. If
#              CRASH_ALLOWED is set to ignore, it will
#              return 0.
##########################################################

function assert_crash_count() {
    local COUNT
    local TOTAL

    log_step "Looking for crashes in logs..."

    TOTAL='0'
    for i in $(seq 1 "$(get_count_of_nodes)"); do
        COUNT=$(cat "$NCTL"/assets/net-1/nodes/node-"$i"/logs/stdout.log 2>/dev/null | grep -w 'previous node instance seems to have crashed' | wc -l)
        TOTAL=$((TOTAL + COUNT))
        log "... node-$i: CRASH COUNT = $COUNT"
    done

    if [ "$CRASH_ALLOWED" = "ignore" ]; then
        log "Crash count ignored by test, skipping $TOTAL crashes..."
        return 0
    fi

    if [ "$CRASH_ALLOWED" != "$TOTAL" ]; then
        log "ERROR: ALLOWED: $CRASH_ALLOWED != TOTAL: $TOTAL"
        exit 1
    fi

    log "SUCCESS: ALLOWED: $CRASH_ALLOWED = TOTAL: $TOTAL"
}

##########################################################
# Function: assert_restart_count
# Uses: Variable RESTARTS_ALLOWED
# Description: Searches the log files counting instances
#              of start ups. Compares count against
#              the total allowed by the test. If
#              RESTARTS_ALLOWED is set to ignore, it will
#              return 0.
##########################################################

function assert_restart_count() {
    local COUNT
    local TOTAL
    local ADJUSTED_RESTARTS_ALLOWED

    log_step "Looking for restarts in logs..."

    TOTAL='0'
    for i in $(seq 1 "$(get_count_of_nodes)"); do
        COUNT=$(cat "$NCTL"/assets/net-1/nodes/node-"$i"/logs/stdout.log 2>/dev/null | grep -w 'node starting up' | wc -l)
        if [ "$COUNT" != "0" ]; then
            # Minus 1 for inital start
            COUNT=$((COUNT - 1))
        fi
        TOTAL=$((TOTAL + COUNT))
        log "... node-$i: RESTART COUNT = $COUNT"
    done

    if [ "$RESTARTS_ALLOWED" = "ignore" ]; then
        log "Restart count ignored by test, skipping $TOTAL restarts..."
        return 0
    fi

    ADJUSTED_RESTARTS_ALLOWED=$((RESTARTS_ALLOWED + RESTARTS_ALLOWED / 2))
    log "Adjusted restarts allowed: $ADJUSTED_RESTARTS_ALLOWED"

    if [ "$TOTAL" -gt "$ADJUSTED_RESTARTS_ALLOWED" ]; then
        log "ERROR: ALLOWED: $ADJUSTED_RESTARTS_ALLOWED < TOTAL: $TOTAL"
        exit 1
    fi

    if [ "$TOTAL" -gt "$RESTARTS_ALLOWED" ]; then
        log "WARN: Test would fail without allowed restart adjustment"
    fi

    log "SUCCESS: ALLOWED: $ADJUSTED_RESTARTS_ALLOWED > TOTAL: $TOTAL"
}

##########################################################
# Function: assert_doppel_count
# Uses: Variable DOPPEL_ALLOWED
# Description: Searches the log files counting instances
#              of doppelgangers. Compares count against
#              the total allowed by the test. If
#              DOPPEL_ALLOWED is set to ignore, it will
#              return 0.
##########################################################

function assert_doppel_count() {
    local COUNT
    local TOTAL

    log_step "Looking for doppels in logs..."

    TOTAL='0'
    for i in $(seq 1 "$(get_count_of_nodes)"); do
        COUNT=$(cat "$NCTL"/assets/net-1/nodes/node-"$i"/logs/stdout.log 2>/dev/null | grep -w "received vertex from a doppelganger" | grep 'SignedWireUnit' | wc -l)
        TOTAL=$((TOTAL + COUNT))
        log "... node-$i: DOPPEL COUNT = $COUNT"
    done

    if [ "$DOPPEL_ALLOWED" = "ignore" ]; then
        log "Doppelganger count ignored by test, skipping $TOTAL doppelgangers..."
        return 0
    fi

    if [ "$DOPPEL_ALLOWED" != "$TOTAL" ]; then
        log "ERROR: ALLOWED: $DOPPEL_ALLOWED != TOTAL: $TOTAL"
        exit 1
    fi

    log "SUCCESS: ALLOWED: $DOPPEL_ALLOWED = TOTAL: $TOTAL"
}

##########################################################
# Function: assert_eject_count
# Uses: Variable EJECTS_ALLOWED
# Description: Searches the log files counting instances
#              of ejections. Compares count against
#              the total allowed by the test. If
#              EJECTS_ALLOWED is set to ignore, it will
#              return 0.
#
# NOTE: Since ejections are global we divide the total
#       by the number of nodes it's found on. This is
#       so we don't confuse ourselves when we expect
#       1 but each node has the message and thus the
#       TOTAL would print 5.
##########################################################

function assert_eject_count() {
    local COUNT
    local TOTAL
    local GLOBAL_DIVIDER

    log_step "Looking for ejections in logs..."

    TOTAL='0'
    GLOBAL_DIVIDER='0'
    for i in $(seq 1 "$(get_count_of_nodes)"); do
        COUNT=$(cat "$NCTL"/assets/net-1/nodes/node-"$i"/logs/stdout.log 2>/dev/null \
                | grep 'era end:' \
                | jq -r '.fields.inactive | select(length > 2)' \
                | tr -d '[()]' \
                | tr ',' '\n' \
                | sed -e 's/^[ \t]*//' \
                | sort \
                | uniq \
                | wc -l)

        if [ "$COUNT" != "0" ]; then
            GLOBAL_DIVIDER=$((GLOBAL_DIVIDER + 1))
        fi

        TOTAL=$((TOTAL + COUNT))
        log "... node-$i: EJECT COUNT = $COUNT"
    done

    if [ "$GLOBAL_DIVIDER" != "0" ]; then
        TOTAL=$((TOTAL / GLOBAL_DIVIDER))
    fi

    if [ "$EJECTS_ALLOWED" = "ignore" ]; then
        log "Ejection count ignored by test, skipping $TOTAL ejections..."
        return 0
    fi

    if [ "$EJECTS_ALLOWED" != "$TOTAL" ]; then
        log "ERROR: ALLOWED: $EJECTS_ALLOWED != TOTAL: $TOTAL"
        exit 1
    fi

    log "SUCCESS: ALLOWED: $EJECTS_ALLOWED = TOTAL: $TOTAL (divided due to global)"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset ERRORS_ALLOWED
unset EQUIVS_ALLOWED
unset DOPPEL_ALLOWED
unset CRASH_ALLOWED
unset RESTARTS_ALLOWED
unset EJECTS_ALLOWED

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        errors) ERRORS_ALLOWED=${VALUE} ;;
        equivocators) EQUIVS_ALLOWED=${VALUE} ;;
        doppels) DOPPEL_ALLOWED=${VALUE} ;;
        crashes) CRASH_ALLOWED=${VALUE} ;;
        restarts) RESTARTS_ALLOWED=${VALUE} ;;
        ejections) EJECTS_ALLOWED=${VALUE} ;;
        *) ;;
    esac
done

ERRORS_ALLOWED=${ERRORS_ALLOWED:-"0"}
EQUIVS_ALLOWED=${EQUIVS_ALLOWED:-"0"}
DOPPEL_ALLOWED=${DOPPEL_ALLOWED:-"0"}
CRASH_ALLOWED=${CRASH_ALLOWED:-"0"}
RESTARTS_ALLOWED=${RESTARTS_ALLOWED:-"0"}
EJECTS_ALLOWED=${EJECTS_ALLOWED:-"0"}

main
