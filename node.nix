let
  moz_overlay = import (builtins.fetchTarball
    "https://github.com/mozilla/nixpkgs-mozilla/archive/master.tar.gz");
  pkgs = import <nixpkgs> { overlays = [ moz_overlay ]; };
  rustChannel = (pkgs.rustChannelOf { rustToolchain = ./rust-toolchain; });
  rustPlatform = pkgs.makeRustPlatform {
    rustc = rustChannel.rust;
    # TODO: Enable for a working development environment.
    # cargo = rustChannel.rust.override { extensions = [ "rust-src" ]; };
    cargo = rustChannel.rust;
  };
  source = pkgs.nix-gitignore.gitignoreSource [ ] ./.;
in rustPlatform.buildRustPackage rec {
  name = "casper-node";
  pname = "casper-node";
  cargoSha256 = "0sv48j9qax6mr96s4i17kb8n4qg9lknc8ban7fsxkjkarx7hrd3x";
  src = source;
  buildInputs = with pkgs; [ openssl ];
  nativeBuildInputs = with pkgs; [ pkg-config ];
  cargoBuildFlags = [ "-p" "casper-node" ];

  # Do not run tests, they require too many dependencies not capture here.
  doCheck = false;

  PROTOC = "${pkgs.protobuf}/bin/protoc";
}
