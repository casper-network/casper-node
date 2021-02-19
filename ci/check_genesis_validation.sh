#!/usr/bin/env bash

set -e

FILE_DIR=$1

function check_for_error()
{
  exit_val=$?
  if [[ $exit_val -ne "0" ]]; then
    echo
    echo "Validation failed for accounts.toml and chainspec.toml in $FILE_DIR"
    echo "Run generate_validation.sh in $FILE_DIR to update validation.md5 and commit."
    echo
  fi
  exit $exit_val
}

trap check_for_error EXIT

cd "$FILE_DIR"
echo "Testing validation.md5 in $FILE_DIR"
md5sum -c validation.md5
