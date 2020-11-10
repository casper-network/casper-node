#!/usr/bin/env bash
set -e

cd /etc/casper

# This will pull latest genesis files down into current directory.
# The expectation is this is installed in and run in /etc/casper with sudo

BRANCH_NAME="release-0.1.0"

BASE_PATH="https://raw.githubusercontent.com/CasperLabs/casper-node/${BRANCH_NAME}/resources/production"
ACCOUNTS_CSV_PATH="${BASE_PATH}/accounts.csv"
CHAINSPEC_TOML_PATH="${BASE_PATH}/chainspec.toml"
VALIDATION_PATH="${BASE_PATH}/validation.md5"

files=("accounts.csv" "chainspec.toml" "validation.md5")
for file in "${files[@]}"; do
  if [[ -f $file ]]; then
    echo "deleting old $file."
    rm "$file"
  fi
done

wget --no-verbose $ACCOUNTS_CSV_PATH
wget --no-verbose $CHAINSPEC_TOML_PATH
wget --no-verbose $VALIDATION_PATH

md5sum -c ./validation.md5
