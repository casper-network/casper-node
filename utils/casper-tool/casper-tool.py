#!/usr/bin/env python3

from datetime import datetime, timedelta
import os

import click
import shutil
import toml

#: List of WASM blobs required to be set up in chainspec.
CONTRACTS = ["mint", "pos", "standard_payment", "auction"]


#: Relative directory to be appended to basedir in case WASM dir is not specified.
DEFAULT_WASM_SUBDIR = ["target", "wasm32-unknown-unknown", "release"]


@click.group()
@click.option(
    "-b",
    "--basedir",
    help="casper-node source code base directory",
    type=click.Path(exists=True, dir_okay=True, file_okay=False, readable=True),
    default=os.path.join(os.path.dirname(__file__), "..", ".."),
)
@click.option(
    "-p",
    "--production",
    is_flag=True,
    help="Use production chainspec template instead of dev/local",
)
@click.option(
    "-C",
    "--chainspec-template",
    type=click.Path(exists=True, dir_okay=False, readable=True),
    help="Chainspec template to use",
)
@click.option(
    "-w",
    "--wasm-dir",
    type=click.Path(exists=True, dir_okay=False, readable=True),
    help="directory containing compiled wasm contracts (defaults to `BASEDIR/{}`".format(
        os.path.join(*DEFAULT_WASM_SUBDIR)
    ),
)
@click.pass_context
def cli(ctx, basedir, production, chainspec_template, wasm_dir):
    """Casper Network creation tool

    Can be used to create new casper-labs chains with automatic validator setups. Useful for testing."""
    obj = {}
    if chainspec_template:
        obj["chainspec_template"] = chainspec_template
    elif production:
        obj["chainspec_template"] = os.path.join(
            basedir, "resources", "production", "chainspec.toml"
        )
    else:
        obj["chainspec_template"] = os.path.join(
            basedir, "resources", "local", "chainspec.toml.in"
        )
    obj["wasm_dir"] = wasm_dir or os.path.join(basedir, *DEFAULT_WASM_SUBDIR)

    ctx.obj = obj
    return


@cli.command("create-network")
@click.pass_obj
@click.argument("target-path", type=click.Path(exists=False, writable=True))
@click.option(
    "-n",
    "--network-name",
    help="The network name (also set in chainspec), defaults to output directory name",
)
@click.option(
    "-g",
    "--genesis-in",
    help="Number of seconds from now until Genesis",
    default=300,
    type=int,
)
def create_network(obj, target_path, network_name, genesis_in):
    if network_name is None:
        network_name = os.path.basename(target_path)

    # Create the network output directories.
    show_val("Output path", target_path)
    os.mkdir(target_path)
    chain_path = os.path.join(target_path, "chain")
    os.mkdir(chain_path)

    # Prepare paths and copy over all contracts.
    show_val("WASM contracts", obj["wasm_dir"])
    contract_paths = {}
    for contract in CONTRACTS:
        key = "{}_installer_path".format(contract)
        basename = "{}.wasm".format(contract)
        source = os.path.join(obj["wasm_dir"], "{}_install.wasm".format(contract))
        target = os.path.join(chain_path, "{}.wasm".format(contract))
        shutil.copy(source, target)

        # We use relative paths when creating a self-contained network.
        contract_paths[contract] = basename

    # Update chainspec values.
    chainspec = create_chainspec(
        obj["chainspec_template"], network_name, genesis_in, contract_paths
    )

    chainspec_path = os.path.join(chain_path, "chainspec.toml")
    toml.dump(chainspec, open(chainspec_path, "w"))
    show_val("Chainspec", chainspec_path)


def create_chainspec(template, network_name, genesis_in, contract_paths):
    """Creates a new chainspec from a template.

    `contract_path` must be a dictionary mapping the keys of `CONTRACTS` to relative or absolute
    paths to be put into the new chainspec.

    Returns a dictionary that can be serialized using `toml`.
    """
    show_val("Chainspec template", template)
    chainspec = toml.load(open(template))

    show_val("Chain name", network_name)
    genesis_timestamp = (datetime.utcnow() + timedelta(seconds=genesis_in)).isoformat(
        "T"
    ) + "Z"

    # Update the chainspec.
    show_val("Genesis", "{} (in {} seconds)".format(genesis_timestamp, genesis_in))
    chainspec["genesis"]["name"] = network_name
    chainspec["genesis"]["timestamp"] = genesis_timestamp

    # Setup WASM contracts.
    for contract in CONTRACTS:
        key = "{}_installer_path".format(contract)
        chainspec["genesis"][key] = contract_paths[contract]

    return chainspec


def show_val(key, value):
    """Auxiliary function to display a value on the terminal."""

    key = "{:>20s}".format(key)
    click.echo("{}:  {}".format(click.style(key, fg="blue"), value))


if __name__ == "__main__":
    cli()
