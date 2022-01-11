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
#   Node commit hash from which to stage.
#   Client commit hash from which to stage.
#######################################

function _main()
{
    local STAGE_ID=${1}
    local PROTOCOL_VERSION=${2}
    local COMMIT_HASHES=${3}
    local NODE_COMMIT_HASH
    local CLIENT_COMMIT_HASH
    local PATH_TO_NODE_REPO
    local PATH_TO_CLIENT_REPO
    local PATH_TO_STAGE

    # Split the two commit hashes into the node and client commit hashes.
    IFS='-' read -ra COMMIT_HASHES_ARRAY <<< "$COMMIT_HASHES"
    NODE_COMMIT_HASH="${COMMIT_HASHES_ARRAY[0]}"
    CLIENT_COMMIT_HASH="${COMMIT_HASHES_ARRAY[1]}"

    PATH_TO_NODE_REPO="$(get_path_to_remotes)/casper-node"
    PATH_TO_CLIENT_REPO="$(get_path_to_remotes)/casper-client-rs"
    PATH_TO_STAGE="$(get_path_to_stage "$STAGE_ID")/$PROTOCOL_VERSION"

    _set_repo "https://github.com/casper-network/casper-node.git" "$PATH_TO_NODE_REPO" "$NODE_COMMIT_HASH"
    _set_repo "https://github.com/casper-ecosystem/casper-client-rs" "$PATH_TO_CLIENT_REPO" "$CLIENT_COMMIT_HASH"
    set_stage_binaries "$PATH_TO_NODE_REPO" "$PATH_TO_CLIENT_REPO"
    set_stage_files_from_repo "$PATH_TO_NODE_REPO" "$PATH_TO_CLIENT_REPO" "$PATH_TO_STAGE"
}

#######################################
# Ensures source code is ready to build.
#######################################
function _set_repo()
{
    local REPO_URL=${1}
    local PATH_TO_REPO=${2}
    local COMMIT_HASH=${3}

    # Clone.
    if [ ! -d "$PATH_TO_REPO" ]; then
        mkdir -p "$PATH_TO_REPO"
        git clone "$REPO_URL" "$PATH_TO_REPO" > /dev/null 2>&1
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
unset COMMIT_HASHES

for ARGUMENT in "$@"
do
    KEY=$(echo "$ARGUMENT" | cut -f1 -d=)
    VALUE=$(echo "$ARGUMENT" | cut -f2 -d=)
    case "$KEY" in
        commits) COMMIT_HASHES=${VALUE} ;;
        stage) STAGE_ID=${VALUE} ;;
        version) PROTOCOL_VERSION=${VALUE} ;;
        *)
    esac
done

_main "${STAGE_ID}" "${PROTOCOL_VERSION}" "$COMMIT_HASHES"
