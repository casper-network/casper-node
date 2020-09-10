# Charlie TestNet Setup

The purpose of this document is to guide configuring a system with the new Rust node for Charlie TestNet.

## Software Install

Initially, we will be distributing the debian package as part of the archive which contained this file.  
The package can be found in the `deb` folder, `casper-node_0.1.x_amd64.deb`.

The package can be installed with `sudo apt install ./deb/casper-node_0.1.x_amd64.deb`.  This will install both
`casper-node` and `casper-client` executables in `/usr/bin`.

Configuration files and other needed files are installed in `/etc/casper/`. And example config file is given
as `/etc/casper/config-example.toml`. This needs to be updated to `config.toml`. You can do a direct copy and 
modify values as needed. We did not distribute `config.toml` so it isn't overwritten when a deb package is 
installed to upgrade.

The `accounts.csv` and `chainspec.toml` files will be installed in `/etc/casper` with the deb package install. 
This should allow easy configuration for each network run. 

## Key Generation

Follow commands in `/etc/casper/validator_keys/README.md` to generate keys and get public key hex to send in.

## systemd

The deb package installs casper-node service unit for systemd.  If you are unfamiliar with systemd, 
the [Arch Linux page on systemd](https://wiki.archlinux.org/index.php/systemd) is a good entry into using it.

Start the casper-node with:

`sudo systemctl start casper-node`

Show status of our system:

`systemctl status casper-node`

```
● casper-node.service - Casper Node
     Loaded: loaded (/lib/systemd/system/casper-node.service; disabled; vendor preset: enabled)
     Active: inactive (dead)
       Docs: https://docs.casperlabs.io

Sep 10 09:08:23 joe-ubuntu systemd[1]: Condition check resulted in Casper Node being skipped.
```

Notice we are not starting because a check condition is not met. This is a result of the `/etc/casper/config.toml`
file being required, but not created yet.

Once created, we can start the service again:

`sudo systemctl start casper-node`
`systemctl status casper-node`

```
Sep 10 09:58:52 joe-ubuntu systemd[1]: Starting Casper Node...
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.210 INFO  [casper_node::cli cli.rs:134] node starting up; version=0.1.0
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.210 WARN  [casper_node::reactor reactor.rs:206] large event size, consider >
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.219 INFO  [casper_node::components::storage::lmdb_store lmdb_store.rs:26] o>
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.220 INFO  [casper_node::components::storage::lmdb_store lmdb_store.rs:26] o>
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.220 INFO  [casper_node::components::storage::lmdb_chainspec_store lmdb_chai>
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.228 INFO  [casper_node::reactor reactor.rs:223] reactor main loop is ready
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.411 INFO  [casper_node::components::chainspec_loader chainspec_loader.rs:14>
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.411 INFO  [casper_node::cli cli.rs:150] finished initialization
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.411 WARN  [casper_node::reactor reactor.rs:206] large event size, consider >
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.416 INFO  [casper_node::components::small_network small_network.rs:170] 8f5>
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.416 INFO  [casper_node::reactor reactor.rs:223] reactor main loop is ready
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.416 INFO  [casper_node::cli cli.rs:162] finished joining
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.416 INFO  [casper_node::components::small_network small_network.rs:662] 8f5>
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.416 WARN  [casper_node::reactor reactor.rs:206] large event size, consider >
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.419 INFO  [casper_node::components::small_network small_network.rs:170] 18e>
Sep 10 09:58:52 joe-ubuntu casper-node[731667]: Sep 10 09:58:52.419 INFO  [casper_node::components::api_server api_server.rs:169] started H>
Sep 10 09:58:52 joe-ubuntu casper-node[731667]:    0: casper_node::panic_hook
Sep 10 09:58:52 joe-ubuntu casper-node[731667]:    1: std::panicking::rust_panic_with_hook
Sep 10 09:58:52 joe-ubuntu casper-node[731667]:              at rustc/0cd7ff7ddfb75a38dca81ad3e76b1e984129e939/src/libstd/panicking.rs:530
Sep 10 09:58:52 joe-ubuntu casper-node[731667]:    2: rust_begin_unwind
Sep 10 09:58:52 joe-ubuntu casper-node[731667]:              at rustc/0cd7ff7ddfb75a38dca81ad3e76b1e984129e939/src/libstd/panicking.rs:437
Sep 10 09:58:52 joe-ubuntu casper-node[731667]:    3: core::panicking::panic_fmt
Sep 10 09:58:52 joe-ubuntu casper-node[731667]:              at rustc/0cd7ff7ddfb75a38dca81ad3e76b1e984129e939/src/libcore/panicking.rs:85
```

Good start with systemd and valid config.toml file. This is throwing a panic as we do not have this node as 
part of the genesis group of nodes in config files. Once all keys are in accounts.csv and pushed back out this 
should start correctly.

An example of logs of a system started correctly:

```
● casper-node.service - Casper Node
     Loaded: loaded (/lib/systemd/system/casper-node.service; disabled; vendor preset: enabled)
     Active: active (running) since Thu 2020-09-10 10:37:14 EDT; 2s ago
       Docs: https://docs.casperlabs.io
   Main PID: 826251 (casper-node)
      Tasks: 26 (limit: 77016)
     Memory: 34.9M
     CGroup: /system.slice/casper-node.service
             └─826251 /usr/bin/casper-node validator /etc/casper/config.toml

Sep 10 10:37:15 joe-ubuntu casper-node[826251]: Sep 10 10:37:15.199 INFO  [casper_node::cli cli.rs:150] finished initialization
Sep 10 10:37:15 joe-ubuntu casper-node[826251]: Sep 10 10:37:15.199 WARN  [casper_node::reactor reactor.rs:206] large event size, consider reducing it or boxing; event_size=184
Sep 10 10:37:15 joe-ubuntu casper-node[826251]: Sep 10 10:37:15.202 INFO  [casper_node::components::small_network small_network.rs:166] 339c..c134: starting server background task; local_address=0.0.0.0:34553; public_address=127.0.0.1:34553
Sep 10 10:37:15 joe-ubuntu casper-node[826251]: Sep 10 10:37:15.202 INFO  [casper_node::reactor reactor.rs:223] reactor main loop is ready
Sep 10 10:37:15 joe-ubuntu casper-node[826251]: Sep 10 10:37:15.202 INFO  [casper_node::cli cli.rs:162] finished joining
Sep 10 10:37:15 joe-ubuntu casper-node[826251]: Sep 10 10:37:15.202 INFO  [casper_node::components::small_network small_network.rs:694] 339c..c134: shutting down socket, no longer accepting incoming connections
Sep 10 10:37:15 joe-ubuntu casper-node[826251]: Sep 10 10:37:15.202 WARN  [casper_node::reactor reactor.rs:206] large event size, consider reducing it or boxing; event_size=544
Sep 10 10:37:15 joe-ubuntu casper-node[826251]: Sep 10 10:37:15.205 INFO  [casper_node::components::small_network small_network.rs:166] b076..384a: starting server background task; local_address=0.0.0.0:34553; public_address=127.0.0.1:34553
Sep 10 10:37:15 joe-ubuntu casper-node[826251]: Sep 10 10:37:15.205 INFO  [casper_node::components::api_server api_server.rs:169] started HTTP server; addr=127.0.0.1:7777
Sep 10 10:37:15 joe-ubuntu casper-node[826251]: Sep 10 10:37:15.205 INFO  [casper_node::reactor reactor.rs:223] reactor main loop is ready
```

### Reading logs

The `journalctl` command is used to read logs from a systemd service. Yow specify the unit name and usually flags
to jump to the end (-x) or follow (-f).

This will show logs at the end of the logs:

`journalctl -u casper-node -x`

This will follow logs as they occur until stopped with Ctrl+C:

`journalctl -u casper-node -f`

### Starting and stopping services

The service will start after installing a new .deb package, assuming `/etc/casper/config.toml` is in place.

To start manually:

`sudo systemctl start casper-node`

To stop:

`sudo systemctl stop casper-node`

