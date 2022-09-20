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

while getopts 'd' opt; do 
    case $opt in
        d ) export NCTL_COMPILE_TARGET="debug";;
        * ) echo "nctl-compile only accepts optional flag -d to compile in debug mode."
    esac
done

if [ "$NCTL_COMPILE_TARGET" = "debug" ]; then
    cargo build --package casper-node
else
    cargo build --release --package casper-node
fi

popd || exit
