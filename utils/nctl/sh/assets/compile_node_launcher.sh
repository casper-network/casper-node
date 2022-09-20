#!/usr/bin/env bash
#
#######################################
# Compiles node launcher software.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_CASPER_NODE_LAUNCHER_HOME - path to casper node launcher repo.
#   NCTL_COMPILE_TARGET - flag indicating whether software compilation target is release | debug.
########################################

# Import utils.
source "$NCTL"/sh/utils/main.sh

pushd "$NCTL_CASPER_NODE_LAUNCHER_HOME" || \
    { echo "Could not find the casper-node-launcher repo - have you cloned it into your working directory?"; exit; }

while getopts 'd' opt; do 
    case $opt in
        d ) export NCTL_COMPILE_TARGET="debug";;
        * ) echo "nctl-compile only accepts optional flag -d to compile in debug mode."
    esac
done

if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
    cargo build
else
    cargo build --release
fi

popd || exit
