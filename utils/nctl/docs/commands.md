# NCTL Commands

## Overview

Upon successful setup, NCTL commands are made available via aliases for execution from within a terminal session.  All such commands are prefixed by `nctl-` and allow you to perform various tasks:

## Node Control Commands

- For setting up a network see [here](commands-setup.md).

- For controlling a network see [here](commands-ctl.md).

## View Commands

- For viewing account information see [here](commands-view-accounts.md).

- For viewing chain information see [here](commands-view-chain.md).

- For viewing node information see [here](commands-view-node.md).

## Deploy Dispatch Commands

- For dispatching simple transfer deploys see [here](commands-deploy-transfers.md).

- For dispatching auction deploys see [here](commands-deploy-auction.md).

- For dispatching ERC-20 deploys see [here](commands-deploy-erc20.md).

## Staging Commands

- For setting up a network from a stage see [here](commands-staging.md).

## Notes

- NOTE 1: all ordinal identifiers are 1 based.

- NOTE 2: all command parameters have default values to simplify the general case of testing a single local network.

- NOTE 3: when executing either the `nctl-interactive` or `nctl-start` commands, the node logging level output can be assigned by passing in the `loglevel` parameter.  If you do not pass in this variable then NCTL defaults either to the current value of RUST_LOG or `debug`.

- NOTE 4: many commands will accept a `node` parameter to determine to which node a query or deploy will be dispatched.  If node=random then a dispatch node is determined JIT.  If node=0 then a single node is chosen for dispatch.
