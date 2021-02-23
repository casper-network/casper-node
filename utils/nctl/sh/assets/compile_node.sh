#!/usr/bin/env bash
#
#######################################
# Compiles node software.
# Globals:
#   NCTL_CASPER_HOME - path to casper node repo.
#   NCTL - path to nctl home directory.
########################################

source "$NCTL"/sh/utils/main.sh

pushd "$NCTL_CASPER_HOME" || exit

if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
    cargo build --package casper-node
else
    cargo build --release --package casper-node
fi

popd || exit
