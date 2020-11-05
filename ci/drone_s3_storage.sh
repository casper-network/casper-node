#!/usr/bin/env bash

set -ex

# This script allows uploading, downloading and purging of files to s3 for sharing between drone pipelines.
#

# Making unique string for temp folder name in S3
# Adding DRONE_REPO to DRONE_BUILD_NUMBER, because build is only unique per repo.
# replacing the / in DRONE_REPO name with _ to not be path in S3

abspath() {
  # generate absolute path from relative path
  # $1     : relative filename
  # return : absolute path
  if [ -d "$1" ]; then
    # dir
    (cd "$1"; pwd)
  elif [ -f "$1" ]; then
    # file
    if [[ $1 == */* ]]; then
      echo "$(cd "${1%/*}"; pwd)/${1##*/}"
    else
      echo "$(pwd)/$1"
    fi
  fi
}

export RUN_DIR=$(dirname $(abspath $0))

DRONE_UNIQUE="${DRONE_BUILD_NUMBER}_${DRONE_REPO/\//_}"

package="drone_s3_storage.sh"
function help {
  echo "$package - store and retrieve artifacts to s3 for use between pipelines"
  echo " "
  echo "$package command [arguments]"
  echo " "
  echo "options:"
  echo "-h, --help "
  echo "put [local source] [s3 target] "
  echo "get [s3 source] [local target] "
  echo "remove [s3 target] "
  echo
  exit 0
}

valid_commands=("put" "get" "del")
ACTION=$1
if [[ " ${valid_commands[*]} " != *" $ACTION "* ]]; then
  echo "Invalid command passed: $ACTION"
  echo "Possible commands are: ${valid_commands[*]}."
  exit 1
fi

if [[ "$ACTION" != "del" ]]; then
  SOURCE=$2
  TARGET=$3

  if [ -z "$SOURCE" ]; then
    echo "Source not provided"
    exit 1
  fi

  if [ -z "$TARGET" ]; then
    echo "Target not provided"
    exit 1
  fi
fi

export CL_S3_BUCKET='casperlabs-cicd-artifacts'
export CL_S3_LOCATION="drone_temp/${DRONE_UNIQUE}"

echo "-H \"X-Vault-Token: $CL_VAULT_TOKEN\"" > ~/.curlrc

if [ ! -d $CL_OUTPUT_S3_DIR ]; then
  mkdir -p "${CL_OUTPUT_S3_DIR}"
fi

# get aws credentials files
export CREDENTIAL_FILE_TMP="$RUN_DIR/s3_vault_output.json"
export CL_VAULT_URL="${CL_VAULT_HOST}/v1/sre/cicd/s3/aws_credentials"
curl -s -q -X GET $CL_VAULT_URL --output $CREDENTIAL_FILE_TMP
if [ ! -f $CREDENTIAL_FILE_TMP ]; then
  echo "[ERROR] Unable to fetch aws credentials from vault: $CL_VAULT_URL"
  exit 1
else
  echo "[INFO] Found credentials file - $CREDENTIAL_FILE_TMP"
  echo "[DEBUG] $(cat $CREDENTIAL_FILE_TMP)"
  # get just the body required by bintray, strip off vault payload
  export AWS_ACCESS_KEY_ID=$(/bin/cat $CREDENTIAL_FILE_TMP | jq -r .data.cicd_agent_to_s3.aws_access_key)
  export AWS_SECRET_ACCESS_KEY=$(/bin/cat $CREDENTIAL_FILE_TMP | jq -r .data.cicd_agent_to_s3.aws_secret_key)
  echo "AWS ACCESS : $AWS_ACCESS_KEY_ID"
fi

case "$ACTION" in
  "put")
    echo "put ${SOURCE} s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${TARGET}"
    s3cmd sync "${SOURCE}" "s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${TARGET}"
    ;;
  "get")
    echo "get s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${SOURCE} ${TARGET}"
    s3cmd sync "s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/${SOURCE}" "${TARGET}"
    ;;
  "del")
    echo "del s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}"
    s3cmd del --recursive "s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}"
    ;;
esac
