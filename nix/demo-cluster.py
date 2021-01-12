#!/usr/bin/env python3

from base64 import b64encode
import os
import time

import click
import kubernetes
import toml
from kubernetes.client.rest import ApiException

TAG = "9bddd925"

#: Prefix for namespaces of deployed networks. Prevents accidental deletion of namespaces like
#  `default`.
NETWORK_NAME_PREFIX = "casper-"


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

    # Upload chainspec as a config map.
    chain_map = load_dir_to_dict(os.path.join(network_path, "chain"))
    api.create_namespaced_config_map(
        namespace=name,
        body={"metadata": {"name": "chainspec",}, "binaryData": chain_map},
    )


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


def load_dir_to_dict(path):
    """Loads all of the contents of a directory into a dict.

    Keys are paths relative to `path`, values the file contents encoded as base64."""

    data = {}

    for dirpath, _dirnames, filenames in os.walk(path):
        for filename in filenames:
            full_path = os.path.join(dirpath, filename)
            relpath = os.path.relpath(full_path, path)
            data[relpath] = b64encode(open(full_path, "rb").read()).decode("ASCII")

    return data


if __name__ == "__main__":
    cli()
