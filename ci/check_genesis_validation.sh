#!/usr/bin/env bash

set -e

FILE_DIR=$1

function error_output()
{
  echo
  echo "Validation failed for accounts.csv and chainspec.toml in $FILE_DIR"
  echo "Run generate_validation.sh in $FILE_DIR to update validation.md5 and commit."
  echo
}

trap 'error_output' EXIT

cd ${FILE_DIR}
echo "Testing validation.md5 in $FILE_DIR"
md5sum -c validation.md5
