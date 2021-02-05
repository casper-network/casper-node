#!/bin/bash

get_help() {
  echo -e "Usage: $0 --repo-name <NAME> --package-name <PACKAGE_NAME> [--package-version <VERSION>]\n"
  echo -e "Example: $0 --repo-name debian --package-name [casper-node|casper-client]\n"
  echo "Note: If --package-version is not set DRONE_TAG will be used."
}

parse_args() {
  if [ $# -eq 0 ]; then
    get_help
    exit 1
  fi
  optspec=":h-:"
  while getopts "$optspec" optchar; do
    case "${optchar}" in
      -)
        case "${OPTARG}" in
          repo-name)
            val="${!OPTIND}"; OPTIND=$(( $OPTIND + 1 ))
            #echo "Parsing option: '--${OPTARG}', value: '${val}'" >&2
            BINTRAY_REPO_NAME=${val}
            ;;
          package-name)
            val="${!OPTIND}"; OPTIND=$(( $OPTIND + 1 ))
            #echo "Parsing option: '--${OPTARG}', value: '${val}'" >&2
            BINTRAY_PACKAGE_NAME=${val}
            ;;
          package-version)
            val="${!OPTIND}"; OPTIND=$(( $OPTIND + 1 ))
            #echo "Parsing option: '--${OPTARG}', value: '${val}'" >&2
            PACKAGE_VERSION=${val}
            ;;
          package-tag)
            val="${!OPTIND}"; OPTIND=$(( $OPTIND + 1 ))
            #echo "Parsing option: '--${OPTARG}', value: '${val}'" >&2
            PACKAGE_TAG=${val}
            ;;
          help)
            get_help
            exit 1
            ;;
          *)
            echo "${optspec:0:1} ${OPTARG}" 
            #if [ "$OPTERR" = 1 ] && [ "${optspec:0:1}" != ":" ]; then
            if [ "$OPTERR" = 1 ]; then
              echo -e "Unknown option --${OPTARG}\n" >&2
              get_help
              exit 1
            fi
            ;;
        esac;;
      h)
        get_help
        exit 1
        ;;
      *)
        if [ "$OPTERR" = 1 ] || [ "${optspec:0:1}" = ":" ]; then
          echo "Non-option argument: '-${OPTARG}'" >&2
          exit 1
        fi
        ;;
    esac
  done
  # obligatory paramas goes here  
  if [ -z ${BINTRAY_REPO_NAME+x} ]; then
    echo "[ERROR] Missing repository name."
    get_help
    exit 1
  fi

  if [ -z ${BINTRAY_PACKAGE_NAME+x} ]; then
    echo "[ERROR] Missing package name."
    get_help
    exit 1
  fi

  # if not set take it from node/Cargo.toml
  if [ -z ${PACKAGE_VERSION+x} ]; then
    NODE_CONFIG_FILE="$RUN_DIR/node/Cargo.toml"
    PACKAGE_VERSION="$(grep -oP "^version\s=\s\"\K(.*)\"" $NODE_CONFIG_FILE | sed -e s'/"//g')"
  fi

  echo "[INFO] PACKAGE_VERSION set to $PACKAGE_VERSION"
}

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
parse_args "$@"

export CREDENTIAL_FILE="$RUN_DIR/credentials.json"
export CREDENTIAL_FILE_TMP="$RUN_DIR/vault_output.json"
export API_URL="https://api.bintray.com"
export UPLOAD_DIR="$(pwd)/target/debian"
export BINTRAY_USER='casperlabs-service'
export BINTRAY_ORG_NAME='casperlabs'
export BINTRAY_REPO_URL="$BINTRAY_ORG_NAME/$BINTRAY_REPO_NAME/$BINTRAY_PACKAGE_NAME"
export CL_VAULT_URL="${CL_VAULT_HOST}/v1/sre/cicd/bintray"

echo "Run dir set to: $RUN_DIR"
echo "Repo URL: $BINTRAY_REPO_URL"
echo "Package version set to: $PACKAGE_VERSION"

# get bintray private key and passphrase
echo "-H \"X-Vault-Token: $CL_VAULT_TOKEN\"" > ~/.curlrc
curl -s -q -X GET $CL_VAULT_URL/credentials --output $CREDENTIAL_FILE_TMP
if [ ! -f $CREDENTIAL_FILE_TMP ]; then
  echo "[ERROR] Unable to fetch credentails for bintray from vault: $CL_VAULT_URL"
  exit 1
else
  echo "Found bintray credentials file - $CREDENTIAL_FILE_TMP"
  # get just the body required by bintray, strip off vault payload
  /bin/cat $CREDENTIAL_FILE_TMP | jq -r .data > $CREDENTIAL_FILE
fi

# get bintray api key
curl -s -q -X GET $CL_VAULT_URL/bintray_api_key --output $CREDENTIAL_FILE_TMP
if [ ! -f $CREDENTIAL_FILE_TMP ]; then
  echo "[ERROR] Unable to fetch api_key for bintray from vault: $CL_VAULT_URL"
  exit 1
else
  echo "Found bintray credentials file - $CREDENTIAL_FILE_TMP"
  # get just the body required by bintray, strip off vault payload
  export BINTRAY_API_KEY=$(/bin/cat $CREDENTIAL_FILE_TMP | jq -r .data.bintray_api_key)
fi

if [ "$PACKAGE_TAG" == "true" ]; then
  REV="0"
else
  REV=${DRONE_BUILD_NUMBER}
fi

if [ -d "$UPLOAD_DIR" ]; then
  DEB_FILE="${BINTRAY_PACKAGE_NAME}_${PACKAGE_VERSION}-${REV}_amd64.deb"
  DEB_FILE_PATH="$UPLOAD_DIR/$DEB_FILE"
else
  echo "[ERROR] Not such dir: $UPLOAD_DIR"
  exit 1
fi

# allow overwrite version for test repo
if [ "$BINTRAY_REPO_NAME" == "casper-debian-tests" ]; then
  echo "[INFO] Setting override=1 for the test repo: $BINTRAY_REPO_NAME"
  export BINTRAY_UPLOAD_URL="$API_URL/content/$BINTRAY_REPO_URL/${PACKAGE_VERSION}/$DEB_FILE?override=1"
else
  export BINTRAY_UPLOAD_URL="$API_URL/content/$BINTRAY_REPO_URL/${PACKAGE_VERSION}/$DEB_FILE"
fi

echo "Uploading file ${DEB_FILE_PATH} to bintray:${PACKAGE_VERSION} ..."
if [ -f "$DEB_FILE_PATH" ]; then
  curl -T $DEB_FILE_PATH -u$BINTRAY_USER:$BINTRAY_API_KEY $BINTRAY_UPLOAD_URL
else
  echo "[ERROR] Unable to find $DEB_FILE_PATH in $(pwd)"
  exit 1
fi

sleep 5 && echo -e "\nPublishing CL Packages on bintray..."
curl -s -X POST -u$BINTRAY_USER:$BINTRAY_API_KEY $API_URL/content/$BINTRAY_REPO_URL/${PACKAGE_VERSION}/publish

sleep 5 && echo -e "\nGPG Signing CL Packages on bintray..."
curl -s -X POST -u$BINTRAY_USER:$BINTRAY_API_KEY -H "Content-Type: application/json" --data "@$CREDENTIAL_FILE" $API_URL/gpg/$BINTRAY_REPO_URL/versions/${PACKAGE_VERSION}

sleep 5 && echo -e "\nPublishing GPG Signatures on bintray..."
curl -s -X POST -u$BINTRAY_USER:$BINTRAY_API_KEY $API_URL/content/$BINTRAY_REPO_URL/${PACKAGE_VERSION}/publish

sleep 5 && echo -e "\nCalculating repo metadata on bintray..."
curl -s -X POST -u$BINTRAY_USER:$BINTRAY_API_KEY -H "Content-Type: application/json" --data "@$CREDENTIAL_FILE" $API_URL/calc_metadata/$BINTRAY_REPO_URL/versions/${PACKAGE_VERSION}

echo -e "\n Fetch meta data for uploaded files"
TEMP_DEB_FILE=uploaded_contents_debian_${PACKAGE_VERSION}.json
curl -s -X GET -u$BINTRAY_USER:$BINTRAY_API_KEY -H "Content-Type: application/json" $API_URL/packages/$BINTRAY_REPO_URL/files?include_unpublished=1 > $TEMP_DEB_FILE

# checking
DEB_FILE_NAME=$(cat $TEMP_DEB_FILE | jq -r 'nth(1; .[] | select (.version == "'${PACKAGE_VERSION}'") ) | .path' )

DEB_ASC_FILE_NAME=$(cat $TEMP_DEB_FILE |  jq -r 'nth(0; .[] | select (.version == "'${PACKAGE_VERSION}'") ) | .path' )

if [[ "$DEB_FILE_NAME" =~ $BINTRAY_PACKAGE_NAME.*.deb$ ]]; then
  echo "Found $DEB_FILE_NAME on bintray";
else
  echo "[ERRROR] Unable to find uploaded packages on bintray - missing $DEB_FILE_NAME"
  exit 1
fi

if [[ "$DEB_ASC_FILE_NAME" =~ $BINTRAY_PACKAGE_NAME.*.deb.asc$ ]]; then
  echo "Found $DEB_ASC_FILE_NAME on bintray";
else
  echo "[ERRROR] Unable to find uploaded packages on bintray - missing $DEB_ASC_FILE_NAME"
  exit 1
fi
