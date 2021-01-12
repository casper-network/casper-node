{ pkgs ? (import <nixpkgs>) { } }:
let
  python = pkgs.python3.withPackages
    (python-packages: with python-packages; [ click kubernetes ]);
in pkgs.mkShell {

  buildInputs = with pkgs; [ skopeo kubectl python black ];

  shellHook = ''
    export KUBECONFIG=$(pwd)/k3s.yaml
  '';

  # Technically not needed for the orchestration tools, but useful when running `casper-tool`.
  PROTOC = "${pkgs.protobuf}/bin/protoc";
}
