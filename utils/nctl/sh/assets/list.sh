#!/usr/bin/env bash
#
# List previously created assets.
# Globals:
#   NCTL - path to nctl home directory.

if [ -d $NCTL/assets ]; then
    ls $NCTL/assets
fi
