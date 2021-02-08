# Nix-based kubernetes test environment

All operations are based on having the `nix` package manager available. It can be easily be installed using the quickstart instructions found at https://nixos.org/download.html#nix-quick-install, which are just

```console
$ curl -L https://nixos.org/nix/install | sh
```

## Building a docker image of a node

To build your current source into a container image, enter a nix-shell in *root folder* of the repository, then run `nix/build-node.sh`.

```console
$ nix-shell
$ nix/build-node.sh
[...]
Created new docker image casper-node:f2b9cd7a-dirty.

Load into local docker:
docker load -i ./result

Publish image
skopeo --insecure-policy copy docker-archive:./result docker://clmarc/casper-node:f2b9cd7a-dirty
```

The image will be inside the nix store as a docker archive file, with a local symlink `result` pointing to it. As shown above, there are now options to either upload to a repository (provided you have credentials), or just import it locally. The image tag will be based on the current state of the source tree, if uncommitted changes to any files are present, a `-dirty` will be appended.

## Setting up a new kubernetes cluster

One option is to use a hosted kubernetes solution, e.g. offerings from Digital Ocean, Amazon or Google. However, hosting a cluster for testing purpose is a cheaper and potentially simpler alternative. We recommend using [k3s](https://k3s.io) to setup a cluster, which is lighter on resources at the cost of not offering high availability for the control plane - a feature not needed for our testing environments.

Here is a brief overview on how to create a cluster on Hetzner's cheap cloud storage (see also the [quick start instructions of k3s](https://rancher.com/docs/k3s/latest/en/quick-start/)):

### Setting up the master node

1. Create any number of nodes, one of which will be the master node.
1. Install k3s on the master using `curl -sfL https://get.k3s.io | sh`.
1. Download the kubeconfig at `/etc/rancher/k3s/k3s.yaml` and make sure to replace the localhost IP in `clusters.cluster.server` with the master node's IP.
1. Make note of the server token in `/var/lib/rancher/k3s/server/node-token`.
1. For any non-master node, run `curl -sfL https://get.k3s.io | K3S_URL=https://${SERVERIP}:6443 K3S_TOKEN=${NODETOKEN} sh -`, replacing `${SERVERIP}` with the master node's IP and `${NODETOKEN}` with the previously mentioned token.

Setting `KUBECONFIG` to the path of the downloaded kubeconfig and running `kubectl get nodes` should show all nodes as online shortly after.

### Enabling storage

The final step is to create a storage provider, for which we will be using Longhorn. This is required to be able to offer persistent storage on nodes, see the [Longhorn example in the k3s docs](https://rancher.com/docs/k3s/latest/en/storage/) for details.

The prerequisite is that the `open-iscsi` is installed **on the node itself**. Thus we enter each node and install it:

```console
# Shortcut to get node IPs
NODE_IPS=$(kubectl get nodes -o 'jsonpath={.items[*].status.addresses[?(@.type=="InternalIP")].address}')
for IP in ${NODE_IPS}; do
    ssh root@$IP "hostname; apt-get -qq update && apt-get -qq install open-iscsi";
done;
# ...
```

With `iscsi` installed, we can now deploy longhorn:

```console
$ kubectl apply -f https://raw.githubusercontent.com/longhorn/longhorn/master/deploy/longhorn.yaml
```
