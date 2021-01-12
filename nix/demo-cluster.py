#!/usr/bin/env python3

import time

import click
import kubernetes
from kubernetes.client.rest import ApiException

NAMESPACE = "testdemo"
TAG = "9bddd925"

def main():
    kubernetes.config.load_kube_config()
    v1 = kubernetes.client.CoreV1Api()

    # Create a new testnetwork


    # Delete all previous data.
    click.echo("Resetting namespace {}".format(NAMESPACE), nl=False)
    delete_namespace(NAMESPACE, v1)
    click.echo()
    v1.create_namespace({"metadata": {"name": NAMESPACE}})



def delete_namespace(name, api):
    watch = kubernetes.watch.Watch()

    # It seems the watch API is not supported for namespace reading? We poll manually.
    while True:
        try:
            ns = api.read_namespace(NAMESPACE)
        except ApiException as e:
            if e.status != 404:
                raise
            return

        click.echo(".", nl=False)
        api.delete_namespace(name)
        time.sleep(0.250)


if __name__ == "__main__":
    main()
