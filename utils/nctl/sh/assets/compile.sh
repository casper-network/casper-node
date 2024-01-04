#!/usr/bin/env bash
#
#######################################
# Compiles software.
# Globals:
#   NCTL - path to nctl home directory.
########################################

unset OPTIND #clean OPTIND envvar, otherwise getopts can break.
COMPILE_MODE="release" #default compile mode to release.

while getopts 'd' opt; do 
    case $opt in
        d )
            COMPILE_MODE="debug"
            ;;
        * )
            
            COMPILE_MODE="release"
            ;; #ignore other cl flags
    esac
done

if [ "$NCTL_COMPILE_TARGET" = "debug" ] || [ "$COMPILE_MODE" == "debug" ]; then
    source "$NCTL"/sh/assets/compile_node.sh -d
    source "$NCTL"/sh/assets/compile_node_launcher.sh -d
    source "$NCTL"/sh/assets/compile_client.sh -d 
    source "$NCTL"/sh/assets/compile_global_state_update_gen.sh -d
    source "$NCTL"/sh/assets/compile_sidecar.sh -d
else
    source "$NCTL"/sh/assets/compile_node.sh
    source "$NCTL"/sh/assets/compile_node_launcher.sh
    source "$NCTL"/sh/assets/compile_client.sh
    source "$NCTL"/sh/assets/compile_global_state_update_gen.sh
    source "$NCTL"/sh/assets/compile_sidecar.sh
fi

unset COMPILE_MODE
unset OPTIND #clean all envvar garbage we may have produced. 