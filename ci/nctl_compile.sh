#!/usr/bin/env bash
set -e

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"
NCTL_CLIENT_BRANCH='dev'

# Activate Environment
pushd "$ROOT_DIR"
source $(pwd)/utils/nctl/activate

# Clone the client and launcher repos if required.
if [ ! -d "$NCTL_CASPER_CLIENT_HOME" ]; then
    echo "Checking out $NCTL_CLIENT_BRANCH of casper-client-rs..."
    git clone -b "$NCTL_CLIENT_BRANCH" https://github.com/casper-ecosystem/casper-client-rs "$NCTL_CASPER_CLIENT_HOME"
fi
if [ ! -d "$NCTL_CASPER_NODE_LAUNCHER_HOME" ]; then
    git clone https://github.com/casper-network/casper-node-launcher "$NCTL_CASPER_NODE_LAUNCHER_HOME"
fi

popd

# NCTL Build
nctl-compile
cachepot --show-stats
