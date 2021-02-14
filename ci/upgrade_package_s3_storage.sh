#!/usr/bin/env bash

set -ex

# This script allows uploading, downloading and purging of files to genesis.casperlabs.io s3 for storing
# possible upgrade package releases to promote to a network or use for testing.

# Using DRONE_BUILD_NUMBER as s3 bucket location

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

if [[ "$ACTION" == "del" ]]; then
  LOCAL=$2

  if [ -z "$LOCAL" ]; then
    echo "Source not provided"
    exit 1
  fi

  if [ -z "$LOCAL" ]; then
    echo "Target not provided"
    exit 1
  fi
fi

CL_S3_BUCKET="genesis.casperlabs.io"
CL_S3_LOCATION="drone"

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

case "$ACTION" in
  "put")
    echo "sync ${LOCAL} s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${DRONE_BUILD_NUMBER}"
    s3cmd sync "${LOCAL}" "s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${DRONE_BUILD_NUMBER}"
    ;;
  "get")
    echo "sync s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${DRONE_BUILD_NUMBER} ${LOCAL}"
    s3cmd sync "s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${DRONE_BUILD_NUMBER}" "${LOCAL}"
    ;;
  "del")
    echo "del --recursive s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${DRONE_BUILD_NUMBER}"
    s3cmd del --recursive "s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${DRONE_BUILD_NUMBER}"
    ;;
esac
