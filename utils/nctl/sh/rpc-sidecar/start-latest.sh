#!/usr/bin/env bash

if [ -z "$NODE_DIR" ]; then
    echo "node_dir must be set"
    exit 1
fi

read SEMVER_CURRENT <<< "$(ls --group-directories-first -td -- $NODE_DIR/bin/* | head -n 1)"
SEMVER_CURRENT=$(basename $SEMVER_CURRENT)

if [ ! -f $NODE_DIR/bin/$SEMVER_CURRENT/casper-rpc-sidecar ]; then
    echo "no sidecar binary to run, exiting"
    exit 0
fi

$NODE_DIR/bin/$SEMVER_CURRENT/casper-rpc-sidecar $NODE_DIR/config/$SEMVER_CURRENT/sidecar.toml
