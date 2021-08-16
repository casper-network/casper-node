#!/usr/bin/env bash
set -e

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"
CONFIG_PATH="$ROOT_DIR/ci/markdown-link-check-config.json"
echo "DEBUG: $ROOT_DIR"
echo "DEBUG: $CONFIG_PATH"
pushd "$ROOT_DIR"

FILES=($(find . -name "*.md" -not -path ".*/node_modules/*"))

for file in "${FILES[@]}"; do
    echo "$file"
    markdown-link-check -v -r -p -c $CONFIG_PATH $file
done
popd
