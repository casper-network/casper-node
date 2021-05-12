#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset PROTOCOL_VERSION

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        version) PROTOCOL_VERSION=${VALUE} ;;
        *)
    esac
done

PROTOCOL_VERSION="${PROTOCOL_VERSION:-"1.0.0"}"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

# Base URL: nctl.
_BASE_URL="http://nctl.casperlabs.io.s3-website.us-east-2.amazonaws.com"

# Set of remote files.
_REMOTE_FILES=(
    "add_bid.wasm"
    "casper-client"
    "casper-node"
    "chainspec.toml.in"
    "config.toml"
    "delegate.wasm"
    "transfer_to_account_u512.wasm"
    "undelegate.wasm"
    "withdraw_bid.wasm"
)

#######################################
# Downloads remote assets for subsequent staging.
# Arguments:
#   Protocol version to be downloaded.
#######################################
function _main()
{
    local PROTOCOL_VERSION=${1}
    local PATH_TO_REMOTE
    local REMOTE_FILE

    PATH_TO_REMOTE="$(get_path_to_remotes)/$PROTOCOL_VERSION"
    if [ -d "$PATH_TO_REMOTE" ]; then
        rm -rf "$PATH_TO_REMOTE"
    fi
    mkdir -p "$PATH_TO_REMOTE"

    pushd "$PATH_TO_REMOTE" || exit
    for REMOTE_FILE in "${_REMOTE_FILES[@]}"
    do
        log "... downloading $PROTOCOL_VERSION :: $REMOTE_FILE"
        curl -O "$_BASE_URL/v$PROTOCOL_VERSION/$REMOTE_FILE" > /dev/null 2>&1
    done
    popd
}

_main "$PROTOCOL_VERSION"
