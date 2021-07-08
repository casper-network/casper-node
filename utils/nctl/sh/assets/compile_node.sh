#!/usr/bin/env bash
#
#######################################
# Compiles node software.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_CASPER_HOME - path to casper node repo.
#   NCTL_COMPILE_TARGET - flag indicating whether software compilation target is release | debug.
########################################

source "$NCTL"/sh/utils/main.sh

pushd "$NCTL_CASPER_HOME" || exit

if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
    cargo build --package casper-node --features enable-legacy-hashing
else
    cargo build --release --package casper-node --features enable-legacy-hashing
fi

popd || exit
