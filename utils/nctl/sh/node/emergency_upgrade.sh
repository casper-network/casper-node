#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh
source "$NCTL"/sh/node/svc_"$NCTL_DAEMON_TYPE".sh
source "$NCTL"/sh/assets/upgrade.sh

unset LOG_LEVEL
unset NODE_ID
unset PROTOCOL_VERSION
unset ACTIVATE_ERA
unset TRUSTED_HASH
unset STATE_SOURCE

for ARGUMENT in "$@"; do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        version) PROTOCOL_VERSION=${VALUE} ;;
        era) ACTIVATE_ERA=${VALUE} ;;
        loglevel) LOG_LEVEL=${VALUE} ;;
        hash) TRUSTED_HASH=${VALUE} ;;
        state-source) STATE_SOURCE=${VALUE} ;;
        *) echo "Unknown argument '${KEY}'. Use 'version', 'era' or 'loglevel'." && exit 1 ;;
    esac
done

LOG_LEVEL=${LOG_LEVEL:-$RUST_LOG}
LOG_LEVEL=${LOG_LEVEL:-debug}
export RUST_LOG=$LOG_LEVEL

STATE_SOURCE=${STATE_SOURCE:-1}

function do_emergency_upgrade() {
    local PROTOCOL_VERSION=${1}
    local ACTIVATE_ERA=${2}
    local TRUSTED_HASH=${3}
    local STATE_SOURCE=${4}
    local NODE_COUNT=${5:-5}
    
    local STATE_SOURCE_PATH=$(get_path_to_node $STATE_SOURCE)

    # Create parameters to the global state update generator.
    # First, we supply the path to the directory of the node whose global state we'll use
    # and the trusted hash.
    local PARAMS
    PARAMS="-d ${STATE_SOURCE_PATH} -h ${TRUSTED_HASH}"

    # Add the parameters that define the new validators.
    # We're using the reserve validators, from NODE_COUNT+1 to NODE_COUNT*2.
    local PATH_TO_NODE
    local PUBKEY
    for NODE_ID in $(seq $((NODE_COUNT + 1)) $((NODE_COUNT * 2))); do
        PATH_TO_NODE=$(get_path_to_node $NODE_ID)
        PUBKEY=`cat "$PATH_TO_NODE"/keys/public_key_hex`
        PARAMS="${PARAMS} -v ${PUBKEY},$(($NODE_ID + 1000000000000000))"
    done

    local PATH_TO_NET=$(get_path_to_net)
    mkdir -p "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"

    # Create the global state update file.
    ln -s "$STATE_SOURCE_PATH"/storage "$STATE_SOURCE_PATH"/global_state
    "$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen $PARAMS \
        > "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml
    rm "$STATE_SOURCE_PATH"/global_state

    # Upgrade all the nodes and copy the global state update file to all the nodes' config dirs.
    for NODE_ID in $(seq 1 $((NODE_COUNT * 2))); do
        PATH_TO_NODE=$(get_path_to_node $NODE_ID)
        _upgrade_node "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$NODE_ID"
        cp "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml \
            "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/global_state.toml
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
do_emergency_upgrade "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$TRUSTED_HASH" "$STATE_SOURCE"
log "emergency upgrade node(s) complete"
