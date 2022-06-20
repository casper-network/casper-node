#!/usr/bin/env bash

#######################################
# Downloads remote assets for subsequent staging.
# Arguments:
#   Protocol version to be downloaded.
#######################################

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Base URL: nctl.
_BASE_URL="http://nctl.casperlabs.io.s3-website.us-east-2.amazonaws.com"

# Set of remote files.
_REMOTE_FILES=($(ls $NCTL/sh/scenarios/configs/upgrade* | xargs -n 1 basename))

function _main()
{
    local PROTOCOL_VERSION=${1}
    local PATH_TO_REMOTE
    local REMOTE_FILE
    local RC_VERSION

    PATH_TO_REMOTE="$(get_path_to_remotes)/$PROTOCOL_VERSION/upgrade_configs"
    mkdir -p "$PATH_TO_REMOTE"

    pushd "$PATH_TO_REMOTE" || exit
    for REMOTE_FILE in "${_REMOTE_FILES[@]}"
    do
        if ( ! curl -Isf "$_BASE_URL/v$PROTOCOL_VERSION/$REMOTE_FILE" > /dev/null 2>&1 ); then
            log "... downloading RC $PROTOCOL_VERSION :: $REMOTE_FILE"
            curl -O "$_BASE_URL/release-$PROTOCOL_VERSION/$REMOTE_FILE" > /dev/null 2>&1
        else
            log "... downloading tagged release $PROTOCOL_VERSION :: $REMOTE_FILE"
            curl -O "$_BASE_URL/v$PROTOCOL_VERSION/$REMOTE_FILE" > /dev/null 2>&1
        fi
    done
    if [ "${#PROTOCOL_VERSION}" = '3' ]; then
        RC_VERSION=$(./casper-node --version | awk '{ print $2 }' |  awk -F'-' '{ print $1 }')
        cd ..
        if [ -d "$(get_path_to_remotes)/$RC_VERSION" ]; then
            rm -rf "$RC_VERSION"
        fi
        mv "$PATH_TO_REMOTE" "$RC_VERSION"
    fi
    popd
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset _PROTOCOL_VERSION

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        version) _PROTOCOL_VERSION=${VALUE} ;;
        *)
    esac
done

_main "$_PROTOCOL_VERSION"
