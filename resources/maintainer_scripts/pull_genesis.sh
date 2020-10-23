#!/usr/bin/env bash
set -e

cd /etc/casper

# This will pull latest genesis files down into current directory.
# The expectation is this is installed in and run in /etc/casper with sudo

branch_name="master"

base_path="https://raw.githubusercontent.com/CasperLabs/casper-node/$branch_name/resources/production"
accounts_csv_path="$base_path/accounts.csv"
chainspec_toml_path="$base_path/chainspec.toml"
validation_path="$base_path/validation.md5"

files=("accounts.csv" "chainspec.toml" "validation.md5")
for file in "${files[@]}"; do
  if [[ -f $file ]]; then
    echo "deleting old $file."
    rm "$file"
  fi
done

wget --no-verbose $accounts_csv_path
wget --no-verbose $chainspec_toml_path
wget --no-verbose $validation_path

md5sum -c ./validation.md5