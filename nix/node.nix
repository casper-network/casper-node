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
  cargoSha256 = "0mzp9r62vx4cayis5fhm0sksr5pn0zly85my30mprrn3dfvwma01";
  src = source;
  buildInputs = with pkgs; [ openssl ];
  nativeBuildInputs = with pkgs; [ pkg-config ];
  cargoBuildFlags = [ "-p" "casper-node" ];

  # Do not run tests, they require too many dependencies not capture here.
  doCheck = false;

  PROTOC = "${pkgs.protobuf}/bin/protoc";
}
