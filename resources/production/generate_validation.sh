#!/usr/bin/env bash

dir=$(cd -P -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)
cd "$dir" || exit

md5sum accounts.csv > validation.md5
md5sum chainspec.toml >> validation.md5
