# NCTL Commands

## Overview

Upon successful setup, NCTL commands are made available via aliases for execution from within a terminal session.  All such commands are prefixed by `nctl-` and allow you to perform various tasks:

- For controlling a network see [here](commands-ctl.md).

- For dispatching deploys to a network see [here](commands-deploys.md).

- For view information related to a network see [here](commands-views.md).

## Notes

- NOTE 1: all network, node & user ordinal identifiers are 1 based.

- NOTE 2: all command parameterrs have default values to simplify the general case of testing a single local network.

- NOTE 3: when executing either the `nctl-interactive` or `nctl-start` commands, the node logging level output can be assigned by passing in the `loglevel` parameter.  If you do not pass in this variable then NCTL defaults either to the current value of RUST_LOG or `debug`.
