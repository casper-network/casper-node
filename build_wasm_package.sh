#!/bin/bash

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
NODE_CONFIG_FILE="$RUN_DIR/node/Cargo.toml"
# have to be sed instead of grep -oP to work in alpine docker image
export WASM_PACKAGE_VERSION="$(grep ^version $NODE_CONFIG_FILE | sed -e s'/.*= "//' | sed -e s'/".*//')"
export CL_WASM_DIR="$RUN_DIR/target/wasm32-unknown-unknown/release"
export CL_OUTPUT_S3_DIR="$RUN_DIR/s3_artifacts/${WASM_PACKAGE_VERSION}"
export CL_WASM_PACKAGE="$CL_OUTPUT_S3_DIR/casper-contracts.tar.gz"
export CL_S3_BUCKET='casperlabs-cicd-artifacts'
export CL_S3_LOCATION="wasm_contracts/${WASM_PACKAGE_VERSION}"

if [ ! -d $CL_OUTPUT_S3_DIR ]; then
  mkdir -p "${CL_OUTPUT_S3_DIR}"
fi
# package all wasm files
echo "[INFO] Checking if wasm files are ready under the path $CL_WASM_DIR"
if [ -d "$CL_WASM_DIR" ]; then
  ls -al $CL_WASM_DIR/*wasm
  echo "[INFO] Creating a tar.gz package: $CL_WASM_PACKAGE"
  pushd $CL_WASM_DIR
  tar zvcf $CL_WASM_PACKAGE *wasm
  popd
else
  echo "[ERROR] No wasm dir: $CL_WASM_DIR"
  exit 1
fi

# upload to s3
if [ -z "$AWS_SECRET_ACCESS_KEY" ] || [ -z "$AWS_ACCESS_KEY_ID" ]; then
    log "ERROR: AWS KEYS needed to run. Contact SRE."
    exit 1
else
    s3cmd put ${CL_WASM_PACKAGE} s3://${CL_S3_BUCKET}/${CL_S3_LOCATION}/casper-contracts.tar.gz
fi
