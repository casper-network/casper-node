# Casper Node Setup

The purpose of this document is to guide configuring a system with the new Rust node to operate within a network.

## Software Install

The package can be installed with `sudo apt install casper-node_x.x.x-x_amd64.deb`.  Depending on system you
might need to use `sudo dpkg -i casper-node_x.x.x-x_amd64.deb`.

This package will install the `casper-node` executable in `/usr/bin`.

Configuration files and other needed files are installed in `/etc/casper/`. An example config file is given
as `/etc/casper/config-example.toml`. This is automatically modified for your system in `config.toml`. If 
`config.toml` exists from a previous install, the file we be created as `config.toml.new`. 

The `accounts.csv` and `chainspec.toml` files will be downloaded into `/etc/casper` with the deb package install. 
If genesis files need to be pulled down again, run `sudo -u casper /etc/casper/pull_genesis.sh`. 

A `casper` user and `casper` group is created during install and used to run the software. 

## External Ports

Port `7777` is needed for http access for the status endpoint: `http://<IP Address>:7777/status`.

Port `34553` is needed for gossip listen with other nodes.

## Key Generation

Follow commands in `/etc/casper/validator_keys/README.md` to generate keys and get public key hex to send in.

## systemd

The deb package installs casper-node service unit for systemd.  If you are unfamiliar with systemd, 
the [Arch Linux page on systemd](https://wiki.archlinux.org/index.php/systemd) is a good intro into using it.

Start the casper-node with:

`sudo systemctl start casper-node`

Show status of our system:

`systemctl status casper-node`

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
Sep 10 10:37:15 joe-ubuntu casper-node[826251]: Sep 10 10:37:15.205 INFO  [casper_node::components::rpc_server rpc_server.rs:169] started HTTP server; addr=127.0.0.1:7777
Sep 10 10:37:15 joe-ubuntu casper-node[826251]: Sep 10 10:37:15.205 INFO  [casper_node::reactor reactor.rs:223] reactor main loop is ready
```

### Reading logs

Logs are created in `/var/log/casper/casper-node.log`.

Log redirection (casper-node stdout to file) is set in the systemd service file (`/lib/systemd/system/casper-node.service`) 
Log rotation is setup in `/etc/logrotate.d/casper-node`.  
If you wish to change log location from `/var/log/casper` you need to change both of these files.

Logs can be viewed with `tail -f /var/log/casper/casper-node.log`. 

### Starting and stopping services

To start service:

`sudo systemctl start casper-node`

To stop:

`sudo systemctl stop casper-node`

## Local Storage

If you need to delete the db for a new run,
you can use the script in `/etc/casper` with `sudo /etc/casper/delete_local_db.sh`.

## Resetting and upgrading for new run

Prior to a new upgrade and run, we need to clean up our local state.

```
sudo systemctl stop casper-node
sudo apt remove casper-node
sudo /etc/casper/delete_local_db.sh
```

Pull down new .deb package, install and run.

```
sudo apt install casper-node_x.x.x-x_amd64.deb`
```
 
Look at `/etc/casper/CHANGE_LOG.md` for any changes that require action from
validators. We might need to update `/etc/casper/config.toml` to changes in the new
`/etc/casper/config-example.toml`.

## Bugs

Please file any bugs as issues on [the casper-node GitHub repo](https://github.com/CasperLabs/casper-node).
