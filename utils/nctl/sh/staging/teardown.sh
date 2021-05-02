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

function _main()
{
    local STAGE_ID=${1}
    local PATH_TO_STAGE

    PATH_TO_STAGE=$(get_path_to_stage "$STAGE_ID")
    if [ -d "$PATH_TO_STAGE" ]; then
        rm -rf "$PATH_TO_STAGE"
    fi

    log "Stage deleted"
}

_main "$STAGE_ID"