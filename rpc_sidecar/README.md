# rpc-sidecar

[![LOGO](https://raw.githubusercontent.com/casper-network/casper-node/master/images/casper-association-logo-primary.svg)](https://casper.network/)

[![Build Status](https://drone-auto-casper-network.casperlabs.io/api/badges/casper-network/casper-node/status.svg?branch=dev)](http://drone-auto-casper-network.casperlabs.io/casper-network/casper-node)
[![Crates.io](https://img.shields.io/crates/v/casper-rpc-sidecar)](https://crates.io/crates/casper-rpc-sidecar)
[![License](https://img.shields.io/badge/license-Apache-blue)](https://github.com/CasperLabs/casper-node/blob/master/LICENSE)

## Synopsis

The sidecar is a process that runs alongside the Casper node and exposes a JSON-RPC interface for interacting with the node. The RPC protocol allows for basic operations like querying global state, sending transactions and deploys etc. All of the RPC methods are documented [here](https://docs.casper.network/developers/json-rpc/).

## Protocol
The sidecar maintains a TCP connection with the node and communicates using a custom binary protocol built on top of [Juliet](https://github.com/casper-network/juliet). The protocol uses a request-response model where the sidecar sends simple self-contained requests and the node responds to them. The requests can be split into these main categories:
- read requests
    - queries for transient in-memory information like the 
      current block height, peer list, component status etc.
    - queries for database items, with both the database and the key 
      always being explicitly specified by the sidecar
- execute transaction requests
    - request to submit a transaction for execution
    - request to speculatively execute a transaction 

The node does not interpret the data it sends where it's not necessary. For example, most database items are sent as opaque byte arrays and the sidecar is responsible for interpreting them. This leaves the sidecar in control of the data it receives and allows it to be more flexible in how it handles it.

## License

Licensed under the [Apache License Version 2.0](https://github.com/casper-network/casper-node/blob/master/LICENSE).
