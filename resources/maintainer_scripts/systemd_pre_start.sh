#!/usr/bin/env bash
set -e

EXIT_CODE=0

# Check for /etc/casper/validator_keys
PATH=/etc/casper/validator_keys/
files=("${PATH}secret_key.pem" "${PATH}public_key.pem" "${PATH}public_key_hex")
for file in "${files[@]}"; do
  if [ ! -f "$file" ]; then
    NEED_KEYS=1
    echo "Expected key file not found: ${file}"
  fi
done

if [[ $NEED_KEYS ]]; then
  echo "Information to generate keys can be found in "${PATH}"README.md."
  EXIT_CODE=1
fi

exit $EXIT_CODE
