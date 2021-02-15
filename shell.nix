# This is an unsupported development environment, instantly enabling `make build
# build-contracts-rs`. There is no official support for this nix derivation.
#
# Do not use this, but follow instructions in the `README.md` instead.
#
# `ops`: enables support for running `casper-tool` and various experimental cluster-based
# testing utilities.
# `dev`: enables tooling useful for development, but not strictly necessary to run/build

{ pkgs ? (import <nixpkgs>) { }, ops ? true, dev ? true }:
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
    with python-packages; [
      click
      kubernetes
      prometheus_client
      psutil
      supervisor
      toml
      volatile
    ]);
in pkgs.stdenv.mkDerivation {
  name = "rustenv";
  nativeBuildInputs = with pkgs; [ pkg-config perl which protobuf ];
  buildInputs = with pkgs;
    [ cmake pkg-config openssl.dev zlib.dev rustup ]
    ++ lib.lists.optionals ops [ kubectl python skopeo git nix ]
    ++ lib.lists.optionals dev [ black podman ];

  # Enable SSL support in pure shells
  SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
  NIX_SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";

  # `protoc` is required but not found by the `prost` crate, unless this envvar is set
  PROTOC = "${pkgs.protobuf}/bin/protoc";

  # Convenient setup when working with k3s clusters.
  shellHook = ''
    NCTL_ACTIVATE="utils/nctl/activate"

    if [ -e nix/k3s.yaml ]; then
      echo "Found k3s.yaml in nix folder, setting KUBECONFIG envvar.";
      export KUBECONFIG=$(pwd)/k3s.yaml
    fi;

    if [ -z "''${NO_NCTL}" ]; then
      if [ -f "''${NCTL_ACTIVATE}" ]; then
        echo "Sourcing ''${NCTL_ACTIVATE}."
        source ''${NCTL_ACTIVATE}
      else
        echo "Warning: ''${NCTL_ACTIVATE} not found and NO_NCTL not set."
      fi;
    else
      echo "NO_NCTL is set, not activating nctl"
    fi;
  '';
}
