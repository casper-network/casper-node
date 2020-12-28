#!/usr/bin/env bash

source "$NCTL"/sh/utils/main.sh

less "$(get_path_to_net)"/chainspec/chainspec.toml
