#!/bin/bash

BONDED_AMOUNT="500000000000000"
ADMIN_INITIAL_BALANCE="500000000000000"

echo > accounts.toml

for vid in {1..5}; do
  casper-client keygen --force validator_$vid
  echo '[[accounts]]' >> accounts.toml
  echo '# '$(casper-client account-address --public-key validator_$vid/public_key.pem) >> accounts.toml
  echo 'public_key = "'$(<validator_$vid/public_key_hex)'"' >> accounts.toml
  echo 'balance = "0"' >> accounts.toml
  echo '[accounts.validator]' >> accounts.toml
  echo 'bonded_amount = "'$BONDED_AMOUNT'"' >> accounts.toml
  echo "" >> accounts.toml
done

casper-client keygen --force admin

echo '[[administrators]]' >> accounts.toml
echo '# '$(casper-client account-address --public-key admin/public_key.pem) >> accounts.toml
echo 'public_key = "'$(<admin/public_key_hex)'"' >> accounts.toml
echo 'balance = "'$ADMIN_INITIAL_BALANCE'"' >> accounts.toml
echo 'weight = 255' >> accounts.toml

cat accounts.toml
