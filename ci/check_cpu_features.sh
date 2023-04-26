#!/bin/sh

# Ensure that there has not been a change in CPU features used.

set -e

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"

cd ${ROOT_DIR}
cargo build --release
utils/dump-cpu-features.sh target/release/casper-node > current-build-cpu-features.txt
diff -u current-build-cpu-features.txt ci/cpu-features-1.4.13-release.txt
echo "Check passed, instruction set extensions in node binary have not been changed since 1.4.13"
