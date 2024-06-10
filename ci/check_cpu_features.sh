#!/usr/bin/env bash

# Ensure that there has not been a change in CPU features used.

set -e

cd $(dirname $0)/..

cargo build --release --bin casper-node
utils/dump-cpu-features.sh target/release/casper-node > current-build-cpu-features.txt
if [[ $(comm -23 current-build-cpu-features.txt ci/cpu-features-1.4.13-release.txt) ]]; then
    exit 1
fi
echo "Check passed, instruction set extensions in node binary have not been changed since 1.4.13"
