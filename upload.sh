#!/bin/bash

export GIT_TAG='0.1.2'
export DRONE_TAG="$GIT_TAG"

export API_URL="https://api.bintray.com"
export UPLOAD_DIR=$(pwd)/artifacts/${DRONE_BRANCH}
export BINTRAY_USER='casperlabs-service'
export BINTRAY_ORG_NAME='casperlabs'
export BINTRAY_REPO_NAME='casper-debian-tests'
export BINTRAY_PACKAGE_NAME='casper-node'
export BINTRAY_REPO_URL="$BINTRAY_ORG_NAME/$BINTRAY_REPO_NAME/$BINTRAY_PACKAGE_NAME"
export CL_VAULT_HOST='http://vault.casperlabs.lan:8200'
export CL_VAULT_URL="$CL_VAULT_HOST/v1/sre/cicd/bintray/credentials"

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
export CREDENTIAL_FILE="$RUN_DIR/credentials.json"
export CREDENTIAL_FILE_TMP="$RUN_DIR/vault_output.json"

echo "Run dir set to: $RUN_DIR"

# get bintray credentials 
echo "-H \"X-Vault-Token: $CL_VAULT_TOKEN\"" > ~/.curlrc
curl -s -q -X GET $CL_VAULT_URL --output $CREDENTIAL_FILE_TMP
if [ ! -f $CREDENTIAL_FILE_TMP ]; then
  echo "[ERROR] Unable to fetch credentails for bintray from vault: $CL_VAULT_URL"
else
  echo "Found bintray credentials file - $CREDENTIAL_FILE_TMP"
  # get just the body required by bintray, strip off vault payload
  /bin/cat $CREDENTIAL_FILE_TMP | jq -r .data > $CREDENTIAL_FILE
fi
cd $UPLOAD_DIR

echo "Uploading file to bintray:${DRONE_TAG} ..."
echo -e "\nDEBIAN" && find . -maxdepth 1 -type f -iregex ".*\\.deb" -printf "%f\n" | xargs -I {} sh -c "echo Attempting to upload [{}] && curl -T {} -u$BINTRAY_USER:$BINTRAY_API_KEY $API_URL/content/$BINTRAY_REPO_URL/${DRONE_TAG}/{} && echo"

# sleep 10 && echo -e "\nPublishing CL Packages on bintray..."
curl -X POST -u$BINTRAY_USER:$BINTRAY_API_KEY $API_URL/content/$BINTRAY_REPO_URL/${DRONE_TAG}/publish

# sleep 10 && echo -e "\nGPG Signing CL Packages on bintray..."
curl -X POST -u$BINTRAY_USER:$BINTRAY_API_KEY -H "Content-Type: application/json" --data "@$CREDENTIAL_FILE" $API_URL/gpg/$BINTRAY_REPO_URL/versions/${DRONE_TAG}

# sleep 10 && echo -e "\nPublishing GPG Signatures on bintray..."
curl -X POST -u$BINTRAY_USER:$BINTRAY_API_KEY $API_URL/content/$BINTRAY_REPO_URL/${DRONE_TAG}/publish

# sleep 10 && echo -e "\nCalculating repo metadata on bintray..."
curl -X POST -u$BINTRAY_USER:$BINTRAY_API_KEY -H "Content-Type: application/json" --data '{"private_key": "'$BINTRAY_PK'", "passphrase": "'$BINTRAY_GPG_PASSPHRASE'"}' $API_URL/calc_metadata/$BINTRAY_REPO_URL/${DRONE_TAG}

TEMP_DEB_FILE=uploaded_contents_debian_${GIT_TAG}.json
curl -s -X GET -u$BINTRAY_USER:$BINTRAY_API_KEY -H "Content-Type: application/json" $API_URL/packages/$BINTRAY_REPO_URL/files?include_unpublished=1 > $TEMP_DEB_FILE.json

cat $TEMP_DEB_FILE.json | jq -r '.[] | select (.version == "'${GIT_TAG}'") | .path'
