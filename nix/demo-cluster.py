#!/usr/bin/env python3

import time

import click
import kubernetes
from kubernetes.client.rest import ApiException

TAG = "9bddd925"

#: Prefix for namespaces of deployed networks. Prevents accidental deletion of namespaces like
#  `default`.
NETWORK_NAME_PREFIX = "casper-"


def k8s():
    kubernetes.config.load_kube_config()
    return kubernetes.client.CoreV1Api()


@click.group()
def cli():
    pass


@cli.command("deploy")
@click.argument("network-path", type=click.Path(exists=True, file_okay=False, dir_okay=True, readable=True))
@click.option("-i", "--id", help="ID for the casper network deployment")
def deploy(network_path, id):
  """Deploy a generated network onto cluster namespace"""

  api = k8s()
  name = NETWORK_NAME_PREFIX + id

  # Ensure we're not overwriting an existing deployment.
  if namespace_exists(api, name):
    click.echo("Kubernetes deployment `{}` already exists. Run `destroy` first.".format(name))
    return


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


def main():
    v1.create_namespace({"metadata": {"name": NAMESPACE}})


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


if __name__ == "__main__":
    cli()
