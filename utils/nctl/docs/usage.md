# NCTL Usage

Once activated, NCTL commands can be used to set up & control nodes within a local test network.  Most NCTL users will be testing a single local network, however developers wishing to test multiple networks in parallel may do so.  This usage guide focuses upon the former use case, i.e. testing a single network, and thus all NCTL commands described below are executed with their default values.  Please refer [here](commands.md) for full details of supported NCTL commands.

## Step 1: Compile network binaries

Prior to testing a network ensure that the binary sets are available:

```
nctl-compile
```

This runs `make setup-rs`, and compiles `casper-node`, `casper-client` and `casper-node-launcher` in release mode.

### NOTE

Ensure you are checked out to the correct branch of each of the three repositories above.  Generally this will
be `dev` (or your working branch recently forked from `dev`) for casper-node and casper-client-rs, and `main` for
casper-node-launcher.

To find out which branch of the client and launcher are compatible with the current branch of node, refer to
[ci.json](https://github.com/casper-network/casper-node/blob/dev/utils/nctl/ci/ci.json).

## Step 2: Create network assets

- Once network binary compilation is complete we need to set up test network assets.  The following command instantiates the full set of assets required to run a 5 node local network with 5 users.  It also creates the assets for a further 5 nodes in order to test join/leave scenarios.  The assets are copied to `$NCTL/assets/net-1`, where $NCTL is the nctl home directory.

```
nctl-assets-setup
```

- Examining the contents of `$NCTL/assets/net-1` you will observe the following (self-explanatory) sub-folders:

```
/bin
/chainspec
/daemon
/faucet
/nodes
/users
```

- Examining the contents of `$NCTL/assets/net-1/nodes/node-1`, i.e. node 1, you will observe the following (self-explanatory) sub-folders:

```
/bin
/config
/keys
/logs
/storage
```

- Examining the contents of `$NCTL/assets/net-1/users/user-1`, i.e. user 1, you will find both cryptographic keys & account public key (hex) files.

- Once assets have been created you are advised to review contents of toml files, i.e. `/chainspec/chainspec.toml` & the various `/nodes/node-X/config/node-config.toml` files.

- If you wish to test modifications to a network's chainspec, you can:

```
vi $NCTL/assets/net-1/chainspec/chainspec.toml
```

- If you wish to test modifications to a node's config, e.g. node 3, you can:

```
vi $NCTL/assets/net-1/nodes/node-3/config/node-config.toml
```

## Step 3: Start a node in interactive mode

- Starting a node interactively is useful to verify that the network assets have been correctly established and that the network is ready for testing.

```
nctl-interactive
```

## Step 4: Start a network in daemon mode

- To start with all or a single node in daemonized mode (this is the preferred operating mode):

```
# Start all nodes in daemon mode.
nctl-start

# Start node 1 in daemon mode.
nctl-start node=1
```

- To view process status of all daemonized nodes:

```
nctl-status
```

- To stop either a single or all daemonized nodes:

```
# Stop all nodes.
nctl-stop

# Stop node 1.
nctl-stop node=1
```

## Step 5: Dump logs & other files

Upon observation of a network behavioral anomaly you can dump relevant assets such as logs & configuration as follows:

```
# Writes dumped files -> $NCTL/dumps/net-1
nctl-assets-dump
```

## Step 6: View information

You can view chain, faucet, node & user information using the set of `nctl-view-*` commands.  See [here](commands.md) for further information.

## Step 7: End testing session

To teardown a network once a testing session is complete:

```
# Delete previously created assets and stops all running nodes.
nctl-assets-teardown
```

## Summary

Using NCTL one can spin up either a single or multiple test networks.  Each network is isolated in terms of its assets - this includes port numbers.  The NCTL commands parameter defaults are set for the general use case of testing a single local 5 node network.  You are encouraged to integrate NCTL into your daily workflow to standardize the manner in which the network is tested in a localized setting.
