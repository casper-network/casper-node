#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Base URL: casper-node-launcher.
_LAUNCHER_URL=http://nctl.casperlabs.io.s3-website.us-east-2.amazonaws.com/casper-node-launcher/casper-node-launcher

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
    log "... downloading launcher"
    curl -O "$_LAUNCHER_URL" > /dev/null 2>&1
    chmod +x ./casper-node-launcher
    popd    

    for PROTOCOL_VERSION in "${_PROTOCOL_VERSIONS[@]}"
    do
        source "$NCTL/sh/staging/set_remote.sh" version=$PROTOCOL_VERSION
    done
}

_main 
