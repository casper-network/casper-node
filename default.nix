# TODO: Download rust version and targets via nix expression instead of rustup.
#
# Enables `make build build-contracts-rs`, not `all`, as we do not include assembly script compiler.

let
  pkgs = import <nixpkgs> { };
  nodejs = pkgs.nodejs_latest;
in pkgs.stdenv.mkDerivation {
  name = "rustenv";
  buildInputs = with pkgs; [
    zlib.dev
    nodejs
    cmake
    pkg-config
    openssl.dev
    protobuf

    # Dependencies just used in makefile
    which
    rustup
    cargo

    # Required to build openssl
    perl
  ];
  shellHook = ''
    export LD_LIBRARY_PATH=${pkgs.zlib}/lib

    # Setup for Makefile
    export NPM=${nodejs}/bin/npm
    export CARGO=${pkgs.rustup}/bin/cargo

    # Setup path so that ASC is found:
    export PATH=$PATH:$(pwd)/smart_contracts/contract_as/node_modules/.bin/

    # Enable SSL support in pure shells
    export SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt

    # `protoc` is required but not found by the `prost` crate, unless this envvar is set
    export PROTOC=${pkgs.protobuf}/bin/protoc
  '';
}
