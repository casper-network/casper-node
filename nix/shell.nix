{ pkgs ? (import <nixpkgs>) { } }:
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
  python = pkgs.python3.withPackages
    (python-packages: with python-packages; [ click kubernetes volatile ]);
in pkgs.mkShell {

  buildInputs = with pkgs; [ skopeo kubectl python black ];

  shellHook = ''
    export KUBECONFIG=$(pwd)/k3s.yaml
  '';

  # Technically not needed for the orchestration tools, but useful when running `casper-tool`.
  PROTOC = "${pkgs.protobuf}/bin/protoc";
}
