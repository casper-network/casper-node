#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

#######################################
# Builds assets for staging from local repo.
# Arguments:
#   Scenario ordinal identifier.
#   Scenario protocol version.
#######################################
function _main()
{
    local STAGE_ID=${1}
    local PROTOCOL_VERSION=${2}
    local PATH_TO_STAGE

    PATH_TO_STAGE="$(get_path_to_stage "$STAGE_ID")/$PROTOCOL_VERSION"
    set_stage_binaries "$NCTL_CASPER_HOME" "$NCTL_CASPER_CLIENT_HOME"
    set_stage_files_from_repo "$NCTL_CASPER_HOME" "$NCTL_CASPER_CLIENT_HOME" "$PATH_TO_STAGE"
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset STAGE_ID
unset PROTOCOL_VERSION

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        stage) STAGE_ID=${VALUE} ;;
        version) PROTOCOL_VERSION=${VALUE} ;;
        *)
    esac
done

_main "${STAGE_ID}" "${PROTOCOL_VERSION}"
