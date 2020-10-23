#!/usr/bin/env bash

exit_code=0

# Check for /etc/casper/validator_keys
path=/etc/casper/validator_keys/
files=($path"secret_key.pem" $path"public_key.pem" $path"public_key_hex")
for file in "${files[@]}"; do
  if [[ -n $file ]]; then
    need_keys=1
    echo "Expected key file not found: $file"
  fi
done

if [[ $need_keys ]]; then
  echo "Information to generate keys can be found in "$path"README.md."
  exit_code=1
fi

# Adding nicer error messages to files require in unit file.
if [ ! -f /etc/casper/config.toml ]; then
  echo "Required file not found: /etc/casper/config.toml"
  echo "Running 'sudo -u casper /etc/casper/config_from_example.sh' will generate this file for you."
  exit_code=1
fi

if [ ! -f /etc/casper/chainspec.toml ]; then
  echo "Required genesis file not found: /etc/casper/chainspec.toml"
  need_genesis=1
fi

if [ ! -f /etc/casper/accounts.csv ]; then
  echo "Required genesis file not found: /etc/casper/accounts.csv"
  need_genesis=1
fi

if [[ $need_genesis ]]; then
  echo "This file can be downloaded and verified with 'sudo -u casper /etc/casper/pull_genesis.sh'."
  exit_code=1
fi

exit $exit_code
