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
# Initialises stage settings file.
# Arguments:
#   Stage ordinal identifier.
#######################################
function _main()
{
    local STAGE_ID=${1}
    local PATH_TO_TEMPLATE
    local PATH_TO_STAGE
    local PATH_TO_STAGE_SETTINGS

    PATH_TO_TEMPLATE="$NCTL/sh/staging/settings-template.txt"
    PATH_TO_STAGE="$(get_path_to_stage "$STAGE_ID")"
    PATH_TO_STAGE_SETTINGS="$PATH_TO_STAGE/settings.sh"

    # Set directory.
    if [ -d "$PATH_TO_STAGE" ]; then
        rm -rf "$PATH_TO_STAGE"
    fi
    mkdir -p "$PATH_TO_STAGE"

    # Set settings.
    cp -r "$PATH_TO_TEMPLATE" "$PATH_TO_STAGE_SETTINGS"

    # Prompt user to edit settings.
    vi $PATH_TO_STAGE_SETTINGS
    log "Stage initialised - please edit settings -> $PATH_TO_STAGE_SETTINGS"
}

_main "$STAGE_ID"