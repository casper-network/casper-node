#!/usr/bin/env bash

#######################################
# Imports
#######################################

source "$NCTL"/sh/utils/main.sh

#######################################
# Upgrades node in the network
# Arguments:
#   Protocol version
#   Era at which new version should be upgraded
#   ID of the node to upgrade
#######################################
function _upgrade_node() {
    local PROTOCOL_VERSION=${1}
    local ACTIVATE_ERA=${2}
    local NODE_ID=${3}

    local PATH_TO_NET
    local PATH_TO_NODE
    local CHUNKED_HASH_ACTIVATION

    CHUNKED_HASH_ACTIVATION="$ACTIVATE_ERA"

    PATH_TO_NET=$(get_path_to_net)

    # Set chainspec file.
    PATH_TO_CHAINSPEC_FILE="$PATH_TO_NET"/chainspec/chainspec.toml
    mkdir -p "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"
    PATH_TO_UPGRADED_CHAINSPEC_FILE="$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/chainspec.toml
    cp "$PATH_TO_CHAINSPEC_FILE" "$PATH_TO_UPGRADED_CHAINSPEC_FILE"

    # Write chainspec contents.
    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_CHAINSPEC_FILE');"
        "cfg['protocol']['version']='$PROTOCOL_VERSION'.replace('_', '.');"
        "cfg['protocol']['activation_point']=$ACTIVATE_ERA;"
        "cfg['protocol']['verifiable_chunked_hash_activation']=$CHUNKED_HASH_ACTIVATION;"
        "toml.dump(cfg, open('$PATH_TO_UPGRADED_CHAINSPEC_FILE', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"

    # Copy casper-node binary.
    PATH_TO_NODE=$(get_path_to_node "$NODE_ID")
    mkdir -p "$PATH_TO_NODE"/bin/"$PROTOCOL_VERSION"

    if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
        cp "$NCTL_CASPER_HOME"/target/debug/casper-node "$PATH_TO_NODE"/bin/"$PROTOCOL_VERSION"/
    else
        cp "$NCTL_CASPER_HOME"/target/release/casper-node "$PATH_TO_NODE"/bin/"$PROTOCOL_VERSION"/
    fi

    # Copy chainspec.
    mkdir -p "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/
    cp "$PATH_TO_UPGRADED_CHAINSPEC_FILE" "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/

    # Copy config file.
    cp $(get_path_to_node_config_file "$NODE_ID") "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/

    # Clean up.
    rm "$PATH_TO_UPGRADED_CHAINSPEC_FILE"
}

#######################################
# Generates a global state update for an emergency upgrade
# Arguments:
#   Protocol version
#   Pre-upgrade global state root hash
#   ID of the node to use as the source of the pre-upgrade global state
#   Number of the nodes in the network
#######################################
function _generate_global_state_update() {
    local PROTOCOL_VERSION=${1}
    local STATE_HASH=${2}
    local STATE_SOURCE=${3:-1}
    local NODE_COUNT=${4:-5}

    local PATH_TO_NET=$(get_path_to_net)

    if [ -f "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml ]; then
        # global state update file exists, no need to generate it again
        return
    fi

    local STATE_SOURCE_PATH=$(get_path_to_node $STATE_SOURCE)

    # Create parameters to the global state update generator.
    # First, we supply the path to the directory of the node whose global state we'll use
    # and the trusted hash.
    local PARAMS
    PARAMS="validators -d ${STATE_SOURCE_PATH}/storage/$(get_chain_name) -s ${STATE_HASH}"

    # Add the parameters that define the new validators.
    # We're using the reserve validators, from NODE_COUNT+1 to NODE_COUNT*2.
    local PATH_TO_NODE
    local PUBKEY
    for NODE_ID in $(seq $((NODE_COUNT + 1)) $((NODE_COUNT * 2))); do
        PATH_TO_NODE=$(get_path_to_node $NODE_ID)
        PUBKEY=`cat "$PATH_TO_NODE"/keys/public_key_hex`
        PARAMS="${PARAMS} -v ${PUBKEY},$(($NODE_ID + 1000000000000000))"
    done

    mkdir -p "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"

    # Create the global state update file.
    "$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen $PARAMS \
        > "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml
}

#######################################
# Performs an emergency upgrade on a node in the network
# Arguments:
#   Protocol version
#   Era at which new version should be upgraded
#   ID of the node to upgrade
#   Pre-upgrade global state root hash
#   ID of a node to be used as the source of the pre-upgrade global state
#   The number of nodes in the network
#######################################
function _emergency_upgrade_node() {
    local PROTOCOL_VERSION=${1}
    local ACTIVATE_ERA=${2}
    local NODE_ID=${3}
    local STATE_HASH=${4}
    local STATE_SOURCE=${5:-1}
    local NODE_COUNT=${6:-5}

    _upgrade_node "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$NODE_ID"

    local PATH_TO_NODE=$(get_path_to_node $NODE_ID)

    # Specify hard reset in the chainspec.
    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_NODE/config/$PROTOCOL_VERSION/chainspec.toml');"
        "cfg['protocol']['hard_reset']=True;"
        "cfg['protocol']['last_emergency_restart']=$ACTIVATE_ERA;"
        "toml.dump(cfg, open('$PATH_TO_NODE/config/$PROTOCOL_VERSION/chainspec.toml', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"

    local PATH_TO_NET=$(get_path_to_net)

    _generate_global_state_update "$PROTOCOL_VERSION" "$STATE_HASH" "$STATE_SOURCE" "$NODE_COUNT"

    cp "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml \
        "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/global_state.toml
}

#######################################
# Generates a global state update for an emergency upgrade performing a balance adjustment
# Arguments:
#   Protocol version
#   Pre-upgrade global state root hash
#   ID of the node to use as the source of the pre-upgrade global state
#   Account hash of the source account
#   Account hash of the target account
#   The amount of motes to be transferred
#   The public key of the proposer
#######################################
function _generate_global_state_update_balances() {
    local PROTOCOL_VERSION=${1}
    local STATE_HASH=${2}
    local STATE_SOURCE=${3:-1}
    local SRC_ACC=${4}
    local TARGET_ACC=${5}
    local AMOUNT=${6}
    local PROPOSER=${7}

    local PATH_TO_NET=$(get_path_to_net)

    if [ -f "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml ]; then
        # global state update file exists, no need to generate it again
        return
    fi

    local STATE_SOURCE_PATH=$(get_path_to_node $STATE_SOURCE)

    # Create parameters to the global state update generator.
    # First, we supply the path to the directory of the node whose global state we'll use
    # and the trusted hash.
    local PARAMS
    PARAMS="balances -d ${STATE_SOURCE_PATH}/storage/$(get_chain_name) -s ${STATE_HASH} -f ${SRC_ACC} -t ${TARGET_ACC} -a ${AMOUNT} -p ${PROPOSER}"

    mkdir -p "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"

    # Create the global state update file.
    "$NCTL_CASPER_HOME"/target/"$NCTL_COMPILE_TARGET"/global-state-update-gen $PARAMS \
        > "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml
}

#######################################
# Performs an emergency upgrade on a node in the network
# Arguments:
#   Protocol version
#   Era at which new version should be upgraded
#   ID of the node to upgrade
#   Pre-upgrade global state root hash
#   ID of a node to be used as the source of the pre-upgrade global state
#   Account hash of the source account
#   Account hash of the target account
#   The amount of motes to be transferred
#   The public key of the proposer
#######################################
function _emergency_upgrade_node_balances() {
    local PROTOCOL_VERSION=${1}
    local ACTIVATE_ERA=${2}
    local NODE_ID=${3}
    local STATE_HASH=${4}
    local STATE_SOURCE=${5:-1}
    local SRC_ACC=${6}
    local TARGET_ACC=${7}
    local AMOUNT=${8}
    local PROPOSER=${9}

    _upgrade_node "$PROTOCOL_VERSION" "$ACTIVATE_ERA" "$NODE_ID"

    local PATH_TO_NODE=$(get_path_to_node $NODE_ID)

    # Specify hard reset in the chainspec.
    local SCRIPT=(
        "import toml;"
        "cfg=toml.load('$PATH_TO_NODE/config/$PROTOCOL_VERSION/chainspec.toml');"
        "cfg['protocol']['hard_reset']=True;"
        "cfg['protocol']['last_emergency_restart']=$ACTIVATE_ERA;"
        "toml.dump(cfg, open('$PATH_TO_NODE/config/$PROTOCOL_VERSION/chainspec.toml', 'w'));"
    )
    python3 -c "${SCRIPT[*]}"

    local PATH_TO_NET=$(get_path_to_net)

    _generate_global_state_update_balances "$PROTOCOL_VERSION" "$STATE_HASH" "$STATE_SOURCE" "$SRC_ACC" "$TARGET_ACC" "$AMOUNT" "$PROPOSER"

    cp "$PATH_TO_NET"/chainspec/"$PROTOCOL_VERSION"/global_state.toml \
        "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/global_state.toml
}
