#!/usr/bin/env bash

# ----------------------------------------------------------------
# IMPORTS
# ----------------------------------------------------------------

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

#######################################
# Builds assets for staging from a commit hash.
# Arguments:
#   Scenario ordinal identifier.
#   Scenario protocol version.
#   Commit hash from which to stage.
#######################################

function _main()
{
    local STAGE_ID=${1}
    local PROTOCOL_VERSION=${2}
    local COMMIT_HASH=${3}
    local PATH_TO_REPO
    local PATH_TO_STAGE

    PATH_TO_REPO="$(get_path_to_remotes)/casper-node"
    PATH_TO_STAGE="$(get_path_to_stage "$STAGE_ID")/$PROTOCOL_VERSION"

    _set_repo "$PATH_TO_REPO" "$COMMIT_HASH"
    set_stage_binaries "$PATH_TO_REPO"
    set_stage_files_from_repo "$PATH_TO_REPO" "$PATH_TO_STAGE"
}

#######################################
# Ensures casper-node source code is ready to build.
#######################################
function _set_repo()
{
    local PATH_TO_REPO=${1}
    local COMMIT_HASH=${2}

    # Clone.
    if [ ! -d "$PATH_TO_REPO" ]; then
        mkdir -p "$PATH_TO_REPO"
        git clone "https://github.com/CasperLabs/casper-node.git" "$PATH_TO_REPO" > /dev/null 2>&1
    fi

    # Fetch remotes & checkout commit.
    pushd "$PATH_TO_REPO" || exit
    git fetch --all > /dev/null 2>&1
    git pull > /dev/null 2>&1
    git checkout "$COMMIT_HASH" > /dev/null 2>&1
    popd || exit
}

# ----------------------------------------------------------------
# ENTRY POINT
# ----------------------------------------------------------------

unset STAGE_ID
unset PROTOCOL_VERSION
unset COMMIT_HASH

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        commit) COMMIT_HASH=${VALUE} ;;
        stage) STAGE_ID=${VALUE} ;;
        version) PROTOCOL_VERSION=${VALUE} ;;
        *)
    esac
done

_main "${STAGE_ID}" "${PROTOCOL_VERSION}" "$COMMIT_HASH"
