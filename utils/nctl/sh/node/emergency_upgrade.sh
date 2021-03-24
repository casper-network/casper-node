#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh
source "$NCTL"/sh/assets/upgrade.sh

unset LOG_LEVEL
unset NODE_ID
unset PROTOCOL_VERSION
unset ACTIVATE_ERA
unset STATE_HASH
unset STATE_SOURCE

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        version) PROTOCOL_VERSION=${VALUE} ;;
        era) ACTIVATE_ERA=${VALUE} ;;
        loglevel) LOG_LEVEL=${VALUE} ;;
        hash) STATE_HASH=${VALUE} ;;
        state-source) STATE_SOURCE=${VALUE} ;;
        *) echo "Unknown argument '${KEY}'. Use 'version', 'era', 'loglevel', 'hash' or 'state-source'." && exit 1 ;;
    esac
done

LOG_LEVEL=${LOG_LEVEL:-$RUST_LOG}
LOG_LEVEL=${LOG_LEVEL:-debug}
export RUST_LOG=$LOG_LEVEL

STATE_SOURCE=${STATE_SOURCE:-1}

function do_emergency_upgrade() {
    local PROTOCOL_VERSION=${1}
    local ACTIVATE_ERA=${2}
    local STATE_HASH=${3}
    local STATE_SOURCE=${4}
    local NODE_COUNT=${5:-5}

    # Upgrade all the nodes and copy the global state update file to all the nodes' config dirs.
    for NODE_ID in $(seq 1 $((NODE_COUNT * 2))); do
        _emergency_upgrade_node "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$NODE_ID" "$STATE_HASH" "$STATE_SOURCE" "$NODE_COUNT"
    done
}

#######################################
# Performs an emergency upgrade on all nodes in the network to a specified protocol version.
# Uses the global state update mechanism to change the validator set to the "reserve" validators.
# Does not modify the chainspec file in any way that has an influence on the network,
# except for setting required entries for the upgrade to take place.
#######################################

#
# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Upgrade node(s).
log "emergency upgrade node(s) begins ... please wait"
do_emergency_upgrade "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$STATE_HASH" "$STATE_SOURCE"
log "emergency upgrade node(s) complete"
