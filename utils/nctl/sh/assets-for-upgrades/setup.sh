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

#######################################
# Prepares assets for upgrade scenario start.
# Arguments:
#   Scenario ordinal identifier.
#######################################
function _main()
{
    local SCENARIO_ID=${1}
    local PATH_TO_SCENARIO="$NCTL/assets-for-upgrades/scenario-$SCENARIO_ID"

    log "upgrade scenario asset setup step 2: STARTS"

    source "$PATH_TO_SCENARIO/settings.sh"

    echo "TODO: setup assets in readiness for network start"

    log "upgrade scenario asset setup step 2: COMPLETE"
}

_main "$SCENARIO_ID"
