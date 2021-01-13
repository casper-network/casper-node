#!/usr/bin/env python3

from base64 import b64encode
import os
import time
import tarfile

import click
import kubernetes
import toml
from kubernetes.client.rest import ApiException
import volatile

TAG = "9bddd925"

#: Prefix for namespaces of deployed networks. Prevents accidental deletion of namespaces like
#  `default`.
NETWORK_NAME_PREFIX = "casper-"

#: Maximum size for a compressed network definition, including WASM.
MAX_CHAIN_MAP_SIZE = 1024 * 1023


def k8s():
    """Loads `KUBECONFIG` and sets up an API client"""
    kubernetes.config.load_kube_config()
    return kubernetes.client.CoreV1Api()


@click.group()
def cli():
    """casper-node Kubernetes cluster administration"""
    pass


@cli.command("deploy")
@click.argument(
    "network-path",
    type=click.Path(exists=True, file_okay=False, dir_okay=True, readable=True),
)
@click.option(
    "-i", "--id", help="ID for the casper network deployment. Defaults to the chainname"
)
def deploy(network_path, id):
    """Deploy a generated network onto cluster namespace"""

    api = k8s()

    if id is None:
        chainspec = toml.load(
            open(os.path.join(network_path, "chain", "chainspec.toml"))
        )
        name = name = NETWORK_NAME_PREFIX + chainspec["genesis"]["name"]
    else:
        name = NETWORK_NAME_PREFIX + id

    # Ensure we're not overwriting an existing deployment.
    if namespace_exists(api, name):
        click.echo(
            "Kubernetes deployment `{}` already exists. Run `destroy` first.".format(
                name
            )
        )
        return

    # Create the namespace.
    api.create_namespace({"metadata": {"name": name}})

    # Since shared volumes are quite complicated, we store our whole network configuration as a
    # config map. This limits the size effectively to < 1 MB, but LZMA-compression should be good
    # enough to bring the size down to < 300 KB, the majority of which are WASM contracts.
    click.echo("Compressing network definition")
    chain_map = b64encode(xz_dir(network_path)).decode("ASCII")

    assert len(chain_map) < MAX_CHAIN_MAP_SIZE

    click.echo("Uploading config map with network definition")
    api.create_namespaced_config_map(
        namespace=name,
        body={
            "metadata": {"name": "chain-map",},
            "binaryData": {"chain_map.tar.xz": chain_map},
        },
    )

    click.echo("Config has been uploaded. You can now deploy the actual network.")


@cli.command("destroy")
@click.argument("name")
def destroy(name):
    """Kill a potentially running network"""

    api = k8s()

    network_name = NETWORK_NAME_PREFIX + name

    if not namespace_exists(api, network_name):
        click.echo("Does not exist: {}".format(network_name))
        return

    click.echo("Deleting namespace {}".format(network_name), nl=False)
    delete_namespace(api, network_name)
    click.echo()


def namespace_exists(api, name):
    """Checks whether a given namespace exists"""
    try:
        ns = api.read_namespace(name)
    except ApiException as e:
        if e.status != 404:
            raise
        return False
    return True


def pod_status(api, namespace, pod_name):
    """Checks whether a given pod exists in a namespace"""
    try:
        pod = api.read_namespaced_pod(name=name, namespace=namespace)
        return resp.status.phase
    except ApiException as e:
        if e.status != 404:
            raise
        return None


def delete_namespace(api, name):
    """Delete a namespace and watch for it to terminate"""
    # It seems the watch API is not supported for namespace reading? We poll manually.
    while True:
        if namespace_exists(api, name):
            try:
                api.delete_namespace(name)
            except ApiException as e:
                if e.status != 404:
                    raise
                break
        else:
            break

        click.echo(".", nl=False)
        time.sleep(0.250)


def xz_dir(target):
    """Creates a `.tar.xz`-formatted archive from a path, with all archive paths being relative to
    the `target` dir.

    Returns the archive in-memory."""
    with volatile.file() as archive:
        with tarfile.open(archive.name, "w:xz") as tar:
            for dirpath, _dirnames, filenames in os.walk(target):
                for filename in filenames:
                    full_path = os.path.join(dirpath, filename)
                    relpath = os.path.relpath(full_path, target)
                    tar.add(full_path, arcname=relpath)

        archive.close()
        return open(archive.name, "rb").read()


if __name__ == "__main__":
    cli()
