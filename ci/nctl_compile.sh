#!/usr/bin/env bash
set -e
shopt -s expand_aliases

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"
JSON_CONFIG_FILE="$ROOT_DIR/ci/ci.json"
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

# Clone external dependencies
for i in "${JSON_KEYS[@]}"; do
    clone_external_repo "$i" "$JSON_CONFIG_FILE"
done

NCTL_HOME="$ROOT_DIR/../casper-nctl"
NCTL_CASPER_HOME="$ROOT_DIR"

if [ ! -d "$NCTL_HOME" ]; then
    echo "ERROR: nctl was not set up correctly, check ci/ci.json, exiting..."
    exit 1
fi

# Activate Environment
pushd "$ROOT_DIR"
source "$NCTL_HOME/activate"
popd

# NCTL Build
nctl-compile
cachepot --show-stats
