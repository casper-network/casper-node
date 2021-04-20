#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

function _main()
{
    local PATH_TO_STAGING="$NCTL/staging"
    local PATH_TO_SCENARIO
    local SCENARIO_VERSION
    local UPGRADE_STAGING_TARGET
    local IFS

    if [ -d "$PATH_TO_STAGING/src" ]; then
        rm -rf "$PATH_TO_STAGING/src"
    fi 

    source "$PATH_TO_STAGING/settings.sh"
    for UPGRADE_STAGING_TARGET in "${NCTL_STAGING_TARGETS[@]}"
    do
        IFS=':'
        read -ra STAGING_TARGET <<< "$UPGRADE_STAGING_TARGET"
        SCENARIO_VERSION="${STAGING_TARGET[0]}"
        PATH_TO_SCENARIO="$PATH_TO_STAGING/$SCENARIO_VERSION"
        if [ -d "$PATH_TO_SCENARIO" ]; then
            rm -rf "$PATH_TO_SCENARIO"
        fi
    done    
}

_main
