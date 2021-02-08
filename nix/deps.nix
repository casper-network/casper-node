# Pinned dependencies for nix packages.

let
  # Fixed mozilla overlay, which does not publish release, so we pin an arbitrary commit.
  moz_overlay = import (builtins.fetchTarball
    "https://github.com/mozilla/nixpkgs-mozilla/archive/8c007b60731c07dd7a052cce508de3bb1ae849b4.tar.gz");

  # Pin to a stable nix release.
  pkgsPath = import (builtins.fetchTarball
    "https://github.com/NixOS/nixpkgs/archive/20.09.tar.gz");
in pkgsPath { overlays = [ moz_overlay ]; }
