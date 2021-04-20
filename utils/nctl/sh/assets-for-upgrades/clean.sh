#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset SCENARIO_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        scenario) SCENARIO_ID=${VALUE} ;;
        *)
    esac
done

SCENARIO_ID=${SCENARIO_ID:-1}

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

function _main()
{
    local SCENARIO_ID=${1}
    local PATH_TO_SCENARIO="$NCTL/assets-for-upgrades/scenario-$SCENARIO_ID"

    if [ -d "$PATH_TO_SCENARIO/src" ]; then
        rm -rf "$PATH_TO_SCENARIO/src"
    fi 
    if [ -d "$PATH_TO_SCENARIO/staging" ]; then
        rm -rf "$PATH_TO_SCENARIO/staging"
    fi 
}

_main "$SCENARIO_ID"
