#!/usr/bin/env bash
set -e

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"
CONFIG_PATH="$ROOT_DIR/ci/markdown-link-check-config.json"
pushd "$ROOT_DIR"

FILES=($(find . -name "*.md" -not -path ".*/node_modules/*"))

for file in "${FILES[@]}"; do
    markdown-link-check -v -r -p -c "$CONFIG_PATH" "$file"
done
popd
