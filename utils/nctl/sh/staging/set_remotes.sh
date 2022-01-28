#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Set of protocol versions.
if [ ! -z "$1" ]; then
    _PROTOCOL_VERSIONS=(${1})
else
    _PROTOCOL_VERSIONS=(
    "1.0.0"
    "1.0.1"
    "1.1.1"
    )
fi

#######################################
# Prepares assets for staging from remotes.
#######################################
function _main()
{
    local PATH_TO_REMOTES=$(get_path_to_remotes)
    if [ ! -d "$PATH_TO_REMOTES" ]; then
        mkdir -p "$PATH_TO_REMOTES"
    fi

    pushd "$PATH_TO_REMOTES" || exit

    log "Using launcher from local copy: $NCTL_CASPER_NODE_LAUNCHER_HOME/target/$NCTL_COMPILE_TARGET/casper-node-launcher"
    cp "$NCTL_CASPER_NODE_LAUNCHER_HOME/target/$NCTL_COMPILE_TARGET/casper-node-launcher" "$(pwd)"
    chmod +x ./casper-node-launcher
    popd

    for PROTOCOL_VERSION in "${_PROTOCOL_VERSIONS[@]}"
    do
        source "$NCTL/sh/staging/set_remote.sh" version=$PROTOCOL_VERSION
    done
}

_main
