# This is an unsupported development environment, instantly enabling `make build
# build-contracts-rs`. There is no official support for this nix derivation.
#
# Do not use this, but follow instructions in the `README.md` instead.

{ pkgs ? (import <nixpkgs>) { } }:
pkgs.stdenv.mkDerivation {
  name = "rustenv";
  nativeBuildInputs = with pkgs; [ pkg-config perl which protobuf ];
  buildInputs = with pkgs; [ cmake pkg-config openssl.dev zlib.dev rustup ];

  # Enable SSL support in pure shells
  SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";

  # `protoc` is required but not found by the `prost` crate, unless this envvar is set
  PROTOC = "${pkgs.protobuf}/bin/protoc";
}
