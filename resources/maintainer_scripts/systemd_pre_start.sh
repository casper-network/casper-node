#!/usr/bin/env bash
set -e

exit_code=0

# Check for /etc/casper/validator_keys
path=/etc/casper/validator_keys/
files=($path"secret_key.pem" $path"public_key.pem" $path"public_key_hex")
for file in "${files[@]}"; do
  if [ ! -f "$file" ]; then
    need_keys=1
    echo "Expected key file not found: $file"
  fi
done

if [[ $need_keys ]]; then
  echo "Information to generate keys can be found in "$path"README.md."
  exit_code=1
fi

exit $exit_code
