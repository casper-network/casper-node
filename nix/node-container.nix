# Container definition for the casper node.
#
# The container uses the node binary as an entrypoint, and expects it `config.toml` at `/`. By
# default, it will launch the node in validator mode.

{ pkgs ? import ./deps.nix, tag ? "latest" }:
let casper-node = (import ./node.nix) { inherit pkgs; };
in pkgs.dockerTools.buildImage {
  name = "casper-node";
  tag = tag;
  contents = with pkgs; [ busybox dnsutils strace ];

  extraCommands = ''
    mkdir -p etc
    echo 'hosts:     files dns' > etc/nsswitch.conf
  '';

  config = {
    Cmd = [ "validator" "/config/node/config.toml" ];
    Entrypoint = [ "${casper-node}/bin/casper-node" ];
    WorkingDir = "/storage";
    Volumes = {
      "/storage" = { };
      "/config" = { };
    };
  };
}
