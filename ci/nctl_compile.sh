#!/usr/bin/env bash
set -e

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"
JSON_CONFIG_FILE="$ROOT_DIR/utils/nctl/ci/ci.json"
JSON_KEYS=($(jq -r '.external_deps | keys[]' "$JSON_CONFIG_FILE"))

function clone_external_repo() {
    local NAME=${1}
    local JSON_FILE=${2}
    local URL
    local BRANCH
    local CLONE_REPO_PATH

    CLONE_REPO_PATH="$ROOT_DIR/../$NAME"
    URL=$(jq -r ".external_deps.\"${NAME}\".github_repo_url" "$JSON_FILE")
    BRANCH=$(jq -r ".external_deps.\"${NAME}\".branch" "$JSON_FILE")

    if [ ! -d "$CLONE_REPO_PATH" ]; then
        echo "... cloning $NAME: branch=$BRANCH"
        git clone -b "$BRANCH" "$URL" "$CLONE_REPO_PATH"
    else
        echo "skipping clone of $NAME: directory already exists."
    fi
}

# Activate Environment
pushd "$ROOT_DIR"
source $(pwd)/utils/nctl/activate
popd

# Clone external dependencies
for i in "${JSON_KEYS[@]}"; do
    clone_external_repo "$i" "$JSON_CONFIG_FILE"
done

# NCTL Build
nctl-compile
cachepot --show-stats
