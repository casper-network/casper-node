#!/usr/bin/env bash
set -ex

echo "$1"/"$2"*.deb
apt install "$1"/target/debian/"$2"*.deb

if ! type "$2" > /dev/null; then
  exit 1
fi

apt remove -y "$2"
