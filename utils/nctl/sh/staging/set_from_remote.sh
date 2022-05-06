#!/usr/bin/env bash

# ----------------------------------------------------------------
# IMPORTS
# ----------------------------------------------------------------

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

#######################################
# Builds assets for staging.
# Arguments:
#   Scenario ordinal identifier.
#   Scenario protocol version.
#######################################
function _main()
{
    local STAGE_ID=${1}
    local PROTOCOL_VERSION=${2}
    local PATH_TO_REMOTE
    local PATH_TO_STAGE

    PATH_TO_REMOTE="$(get_path_to_remotes)/$(get_protocol_version_for_chainspec "$PROTOCOL_VERSION")"
    if [ ! -d "$PATH_TO_REMOTE" ]; then
        log "Error: remote assets for protocol version $PROTOCOL_VERSION have not been downloaded"
        exit 1
    fi

    if [ "$PROTOCOL_VERSION" = "1_1_1" ]; then
        log "... NOTE: Setting up 1_1_1 in 1_1_0 due to known caveat."
        PATH_TO_STAGE="$(get_path_to_stage "$STAGE_ID")/1_1_0"
        mkdir -p "$PATH_TO_STAGE"
        rmdir "$(get_path_to_stage "$STAGE_ID")/1_1_1"
    else
        PATH_TO_STAGE="$(get_path_to_stage "$STAGE_ID")/$PROTOCOL_VERSION"
    fi

    cp -r "$PATH_TO_REMOTE"/* "$PATH_TO_STAGE"
    cp "$(get_path_to_remotes)/casper-node-launcher" "$PATH_TO_STAGE"
    mv "$PATH_TO_STAGE/chainspec.toml.in" "$PATH_TO_STAGE/chainspec.toml"
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
