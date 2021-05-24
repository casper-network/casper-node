#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset STAGE_ID

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        stage) STAGE_ID=${VALUE} ;;
        *)
    esac
done

STAGE_ID="${STAGE_ID:-1}"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

#######################################
# Prepares assets for staging.
# Arguments:
#   Scenario ordinal identifier.
#######################################
function _main()
{
    local STAGE_ID=${1}
    local PROTOCOL_VERSION
    local ASSET_SOURCE
    local STAGE_TARGET
    local IFS=':'

    # Import stage settings.
    source "$(get_path_to_stage_settings "$STAGE_ID")"    

    # For each protocol version build a stage.
    for STAGE_TARGET in "${NCTL_STAGE_TARGETS[@]}"
    do
        read -ra STAGE_TARGET <<< "$STAGE_TARGET"
        PROTOCOL_VERSION="${STAGE_TARGET[0]}"
        ASSET_SOURCE="${STAGE_TARGET[1]}"
        source "$NCTL/sh/staging/build.sh" stage="$STAGE_ID" \
                                           version="$PROTOCOL_VERSION" \
                                           source="$ASSET_SOURCE"
    done
}

_main "$STAGE_ID"
