#!/usr/bin/env bash
set -ex

apt install "$DRONE_DIR"/"$1"*.deb

if ! type "$1" > /dev/null; then
  exit 1
fi

apt remove "$1"
