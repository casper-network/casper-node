#!/bin/bash

get_help() {
  echo -e "Usage: $0 --repo-name <NAME> --package-name <PACKAGE_NAME> [--package-version <VERSION>]\n"
  echo -e "Example: $0 --repo-name debian --package-name casper-node\n"
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

  # if not set take it from git tag, should be default behavior.
  if [ -z ${PACKAGE_VERSION+x} ]; then
    export PACKAGE_VERSION="${DRONE_TAG}"
  fi
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

parse_args "$@"

export RUN_DIR=$(dirname $(abspath $0))
export CREDENTIAL_FILE="$RUN_DIR/credentials.json"
export CREDENTIAL_FILE_TMP="$RUN_DIR/vault_output.json"
export API_URL="https://api.bintray.com"
export UPLOAD_DIR="$(pwd)/target/debian"
export BINTRAY_USER='casperlabs-service'
export BINTRAY_ORG_NAME='casperlabs'
export BINTRAY_REPO_URL="$BINTRAY_ORG_NAME/$BINTRAY_REPO_NAME/$BINTRAY_PACKAGE_NAME"
export CL_VAULT_URL="${CL_VAULT_HOST}/v1/sre/cicd/bintray/credentials"

echo "Run dir set to: $RUN_DIR"
echo "Repo URL: $BINTRAY_REPO_URL"
echo "Package version set to: $PACKAGE_VERSION"

# get bintray credentials 
echo "-H \"X-Vault-Token: $CL_VAULT_TOKEN\"" > ~/.curlrc
curl -s -q -X GET $CL_VAULT_URL --output $CREDENTIAL_FILE_TMP
if [ ! -f $CREDENTIAL_FILE_TMP ]; then
  echo "[ERROR] Unable to fetch credentails for bintray from vault: $CL_VAULT_URL"
  exit 1
else
  echo "Found bintray credentials file - $CREDENTIAL_FILE_TMP"
  # get just the body required by bintray, strip off vault payload
  /bin/cat $CREDENTIAL_FILE_TMP | jq -r .data > $CREDENTIAL_FILE
fi

if [ -d "$UPLOAD_DIR" ]; then
  cd $UPLOAD_DIR
else
  echo "[ERROR] Not such dir: $UPLOAD_DIR"
  exit 1
fi

echo "Uploading file to bintray:${DRONE_TAG} ..."
echo -e "\nDEBIAN" && find . -maxdepth 1 -type f -iregex "casper-node.*\\.deb" -printf "%f\n" | xargs -I {} sh -c "echo Attempting to upload [{}] && curl -T {} -u$BINTRAY_USER:$BINTRAY_API_KEY $API_URL/content/$BINTRAY_REPO_URL/${PACKAGE_VERSION}/{} && echo"

# sleep 10 && echo -e "\nPublishing CL Packages on bintray..."
curl -s -X POST -u$BINTRAY_USER:$BINTRAY_API_KEY $API_URL/content/$BINTRAY_REPO_URL/${PACAKGE_VERSION}/publish

# sleep 10 && echo -e "\nGPG Signing CL Packages on bintray..."
curl -s -X POST -u$BINTRAY_USER:$BINTRAY_API_KEY -H "Content-Type: application/json" --data "@$CREDENTIAL_FILE" $API_URL/gpg/$BINTRAY_REPO_URL/versions/${PACKAGE_VERSION}

# sleep 10 && echo -e "\nPublishing GPG Signatures on bintray..."
curl -s -X POST -u$BINTRAY_USER:$BINTRAY_API_KEY $API_URL/content/$BINTRAY_REPO_URL/${PACKAGE_VERSION}/publish

# sleep 10 && echo -e "\nCalculating repo metadata on bintray..."
curl -s -X POST -u$BINTRAY_USER:$BINTRAY_API_KEY -H "Content-Type: application/json" --data '{"private_key": "'$BINTRAY_PK'", "passphrase": "'$BINTRAY_GPG_PASSPHRASE'"}' $API_URL/calc_metadata/$BINTRAY_REPO_URL/${PACKAGE_VERSION}

TEMP_DEB_FILE=uploaded_contents_debian_${PACKAGE_VERSION}.json
curl -s -X GET -u$BINTRAY_USER:$BINTRAY_API_KEY -H "Content-Type: application/json" $API_URL/packages/$BINTRAY_REPO_URL/files?include_unpublished=1 > $TEMP_DEB_FILE.json

cat $TEMP_DEB_FILE.json | jq -r '.[] | select (.version == "'${PACKAGE_VERSION}'") | .path'
