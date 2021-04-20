#!/usr/bin/env bash

source "$NCTL/sh/utils/main.sh"

# ----------------------------------------------------------------
# MAIN
# ----------------------------------------------------------------

function _main()
{
    local PATH_TO_TEMPLATE="$NCTL/sh/staging/template-settings.txt"
    local PATH_TO_STAGING="$NCTL/staging"

    # Set directory.
    if [ -d "$PATH_TO_STAGING" ]; then
        rm -rf "$PATH_TO_STAGING"
    fi
    mkdir -p "$PATH_TO_STAGING"

    # Set settings.
    cp -r "$PATH_TO_TEMPLATE" "$PATH_TO_STAGING/settings.sh"

    # Prompt user to edit settings.
    vi "$PATH_TO_STAGING/settings.sh" 
}

_main