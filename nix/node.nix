{ pkgs ? import ./deps.nix }:
let
  rustChannel = (pkgs.rustChannelOf { rustToolchain = ../rust-toolchain; });
  rustPlatform = pkgs.makeRustPlatform {
    rustc = rustChannel.rust;
    cargo = rustChannel.rust;
  };
  source = pkgs.nix-gitignore.gitignoreSource [ "nix/" ] ../.;
in rustPlatform.buildRustPackage rec {
  name = "casper-node";
  pname = "casper-node";
  cargoSha256 = "1v7did47ckmmlkrkp6rd5r4gqjrl22gm5zwi710w66lr00zvc092";
  src = source;
  buildInputs = with pkgs; [ openssl ];
  nativeBuildInputs = with pkgs; [ pkg-config ];
  cargoBuildFlags = [ "-p" "casper-node" ];

  # Do not run tests, they require too many dependencies not capture here.
  doCheck = false;

  PROTOC = "${pkgs.protobuf}/bin/protoc";
}
