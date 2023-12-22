#!/usr/bin/env bash

if [ -z "$NODE_DIR" ]; then
    echo "node_dir must be set"
    exit 1
fi

read SEMVER_CURRENT <<< "$(ls -d $NODE_DIR/bin/*/ | sort -rV | head -n 1)"
SEMVER_CURRENT=$(basename $SEMVER_CURRENT)

if [ ! -f $NODE_DIR/bin/$SEMVER_CURRENT/casper-rpc-sidecar ]; then
    echo "no sidecar binary to run, exiting"
    exit 0
fi

$NODE_DIR/bin/$SEMVER_CURRENT/casper-rpc-sidecar $NODE_DIR/config/$SEMVER_CURRENT/sidecar.toml
