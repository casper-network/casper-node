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
    cp "$PATH_TO_NODE"/config/1_0_0/config.toml "$PATH_TO_NODE"/config/"$PROTOCOL_VERSION"/

    # Clean up.
    rm "$PATH_TO_UPGRADED_CHAINSPEC_FILE"
}
