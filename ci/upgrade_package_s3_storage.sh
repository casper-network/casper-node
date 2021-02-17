#!/usr/bin/env bash

set -e

# This script allows uploading, downloading and purging of files to genesis.casperlabs.io s3 for storing
# possible upgrade package releases to promote to a network or use for testing.

# Using drone/GIT_HASH/PROTOCOL_VERSION as s3 bucket location in genesis.casperlabs.io

# Check python has toml for getting PROTOCOL_VERSION
set +e
python3 -c "import toml" 2>/dev/null
if [[ $? -ne 0 ]]; then
  echo "Ensure you have 'toml' installed for Python3"
  echo "e.g. run"
  echo "    python3 -m pip install toml --user"
  echo ""
  exit 3
fi
set -e

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." >/dev/null 2>&1 && pwd)"
GENESIS_FILES_DIR="$ROOT_DIR/resources/production"

PROTOCOL_VERSION=$(cat "$GENESIS_FILES_DIR/chainspec.toml" | python3 -c "import sys, toml; print(toml.load(sys.stdin)['protocol']['version'].replace('.','_'))")
echo "Protocol version: $PROTOCOL_VERSION"
GIT_HASH=$(git rev-parse HEAD)
echo "Git hash: $GIT_HASH"

valid_commands=("put" "get" "del")
ACTION=$1
if [[ " ${valid_commands[*]} " != *" $ACTION "* ]]; then
  echo "Invalid command passed: $ACTION"
  echo "Possible commands are:"
  echo " put <local source with ending />"
  echo " get <local target>"
  echo " del "
  exit 1
fi

if [[ "$ACTION" != "del" ]]; then
  LOCAL=$2

  if [ -z "$LOCAL" ]; then
    echo "Local path not provided"
    exit 1
  fi
fi

echo "CL_VAULT_TOKEN: '${CL_VAULT_TOKEN}'"
echo "CL_VAULT_HOST: '${CL_VAULT_HOST}'"
# get aws credentials files
CL_VAULT_URL="${CL_VAULT_HOST}/v1/sre/cicd/s3/aws_credentials"
CREDENTIALS=$(curl -s -q -H "X-Vault-Token: $CL_VAULT_TOKEN" -X GET "$CL_VAULT_URL")
# get just the body required by s3cmd, strip off vault payload
AWS_ACCESS_KEY_ID=$(echo "$CREDENTIALS" | jq -r .data.cicd_agent_to_s3.aws_access_key)
export AWS_ACCESS_KEY_ID
AWS_SECRET_ACCESS_KEY=$(echo "$CREDENTIALS" | jq -r .data.cicd_agent_to_s3.aws_secret_key)
export AWS_SECRET_ACCESS_KEY

CL_S3_BUCKET="genesis.casperlabs.io"
CL_S3_LOCATION="drone/$GIT_HASH"

case "$ACTION" in
"put")
  echo "sync ${LOCAL} s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${PROTOCOL_VERSION}/"
  s3cmd sync "${LOCAL}" "s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${PROTOCOL_VERSION}/"
  ;;
"get")
  echo "sync s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${PROTOCOL_VERSION}/ ${LOCAL}"
  s3cmd sync "s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${PROTOCOL_VERSION}/" "${LOCAL}"
  ;;
"del")
  echo "del --recursive s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}"
  s3cmd del --recursive "s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}"
  ;;
esac
