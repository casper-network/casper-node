{ pkgs ? (import <nixpkgs>) { } }:
pkgs.mkShell {
  buildInputs = with pkgs; [ skopeo kubectl ];

  shellHook = ''
    export KUBECONFIG=$(pwd)/k3s.yaml
  '';
}
