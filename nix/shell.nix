{ pkgs ? (import <nixpkgs>) { } }:
let
  python = pkgs.python3.withPackages
    (python-packages: with python-packages; [ click kubernetes ]);
in pkgs.mkShell {

  buildInputs = with pkgs; [ skopeo kubectl python black ];

  shellHook = ''
    export KUBECONFIG=$(pwd)/k3s.yaml
  '';
}
