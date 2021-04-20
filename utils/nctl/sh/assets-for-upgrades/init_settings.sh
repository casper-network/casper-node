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
    local PATH_TO_TEMPLATE="$NCTL/sh/assets-for-upgrades/template-settings.txt"
    local PATH_TO_SCENARIO="$NCTL/assets-for-upgrades/scenario-$SCENARIO_ID"

    # Set directory.
    if [ -d "$PATH_TO_SCENARIO" ]; then
        rm -rf "$PATH_TO_SCENARIO"
    fi
    mkdir -p "$PATH_TO_SCENARIO"

    # Set settings.
    cp -r "$PATH_TO_TEMPLATE" "$PATH_TO_SCENARIO/settings.sh"

    # Prompt user to edit settings.
    vi "$PATH_TO_SCENARIO/settings.sh" 
}

_main "$SCENARIO_ID"
