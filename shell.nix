# This is an unsupported development environment, instantly enabling `make build
# build-contracts-rs`. There is no official support for this nix derivation.
#
# Do not use this, but follow instructions in the `README.md` instead.
#
# `ops`: enables support for running `casper-tool` and various experimental cluster-based
# testing utilities.
# `dev`: enables tooling useful for development, but not strictly necessary to run/build

{ pkgs ? (import <nixpkgs>) { }, ops ? true, dev ? true }:
with pkgs.lib;
let
  # `volatile` is not packaged in nix.
  volatile = pkgs.python38Packages.buildPythonPackage rec {
    pname = "volatile";
    version = "2.1.0";
    src = pkgs.python38Packages.fetchPypi {
      inherit pname version;
      sha256 = "1lri7a6pmlx9ghbrsgd702c3n862glwy0p8idh0lwdg313anmqwv";
    };
    doCheck = false;
  };
  python = pkgs.python3.withPackages (python-packages:
    with python-packages;
    [ click ] ++ lists.optionals ops [ kubernetes volatile ]
    ++ lists.optionals dev [ prometheus_client psutil supervisor toml ]);
  run-nctl = pkgs.writeScriptBin "nctl" ''
    #!${pkgs.bash}/bin/bash
    COMMAND_LINE="nctl-$@"

    shopt -s expand_aliases
    source ''${CASPER_ROOT}/utils/nctl/activate

    eval ''${COMMAND_LINE}
  '';
in pkgs.stdenv.mkDerivation {
  name = "rustenv";
  nativeBuildInputs = with pkgs; [ pkg-config perl which protobuf ];
  buildInputs = with pkgs;
    [ cmake pkg-config openssl.dev zlib.dev rustup ]
    ++ lists.optionals ops [ kubectl python skopeo git nix ]
    ++ lists.optionals dev [ black podman coreutils run-nctl ];

  # Enable SSL support in pure shells
  SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
  NIX_SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";

  # `protoc` is required but not found by the `prost` crate, unless this envvar is set
  PROTOC = "${pkgs.protobuf}/bin/protoc";

  # The shell hook provides a predefined environment with kubectl and nctl setup, if `ops` and `dev`
  # respectively are enabled.
  shellHook = let
    devS = boolToString dev;
    opsS = boolToString ops;
  in ''
    NCTL_ACTIVATE="utils/nctl/activate"

    if [ ${opsS} = "true" ] && [ -e nix/k3s.yaml ]; then
      echo "Found k3s.yaml in nix folder, setting KUBECONFIG envvar.";
      export KUBECONFIG=$(pwd)/k3s.yaml
    fi;

    if [ ${devS} = "true" ]; then
      if [ -f "''${NCTL_ACTIVATE}" ]; then
        echo "Sourcing ''${NCTL_ACTIVATE}."
        source ''${NCTL_ACTIVATE}
      else
        echo "Warning: ''${NCTL_ACTIVATE} not found."
      fi;
    fi;

    export PS1="\n\[\033[1;32m\][casper-sh:\w]\$\[\033[0m\] ";

    if [ $(pwd | wc -c) -gt 50 ]; then
      echo ""
      echo "WARNING"
      echo "The current path $(pwd) is very long. This will cause issues with UNIX sockets"
      echo "(see https://stackoverflow.com/questions/34829600/why-is-the-maximal-path-length-allowed-for-unix-sockets-on-linux-108)."
      echo
      echo "Consider moving or symlinking this directory."
    fi

    export CASPER_ROOT=$(pwd)
  '';
}
