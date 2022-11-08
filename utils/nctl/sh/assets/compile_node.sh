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

unset OPTIND #clean OPTIND envvar, otherwise getopts can break.
COMPILE_MODE="release" #default compile mode to release.

while getopts 'd' opt; do
    case $opt in
        d ) 
            COMPILE_MODE="debug"
            ;;
        * ) 
            ;; #ignore other cl flags
    esac
done

if [ "$NCTL_COMPILE_TARGET" = "debug" ] || [ "$COMPILE_MODE" = "debug" ]; then
    cargo build --package casper-node
else
    cargo build --release --package casper-node
fi

unset OPTIND
unset COMPILE_MODE #clean all envvar garbage we may have produced. 

popd || exit
