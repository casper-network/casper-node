#!/usr/bin/env bash
#
#######################################
# Compiles global-state-update-gen for emergency upgrades testing.
# Globals:
#   NCTL - path to nctl home directory.
#   NCTL_CASPER_HOME - path to casper node repo.
########################################

source "$NCTL"/sh/utils/main.sh

pushd "$NCTL_CASPER_HOME/utils/global-state-update-gen" || exit

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
