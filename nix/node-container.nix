# Container definition for the casper node.
#
# The container uses the node binary as an entrypoint, and expects it `config.toml` at `/`. By
# default, it will launch the node in validator mode.

{ pkgs ? (import <nixpkgs>) { }, tag ? "latest" }:
let casper-node = (import ./node.nix) { };
in pkgs.dockerTools.buildImage {
  name = "casper-node";
  tag = tag;
  config = {
    Cmd = [ "validator" "/config/config.toml" ];
    Entrypoint = [ "${casper-node}/bin/casper-node" ];
    WorkingDir = "/storage";
    Volumes = {
      "/storage" = { };
      "/config" = { };
    };
  };
}
