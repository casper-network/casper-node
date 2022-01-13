# Changelog

All notable changes to this project will be documented in this file.  The format is based on [Keep a Changelog].

[comment]: <> (Added:      new features)
[comment]: <> (Changed:    changes in existing functionality)
[comment]: <> (Deprecated: soon-to-be removed features)
[comment]: <> (Removed:    now removed features)
[comment]: <> (Fixed:      any bug fixes)
[comment]: <> (Security:   in case of vulnerabilities)


## [Unreleased]

### Added
* Add ability to force DB integrity checks to run on node start by setting env var `CL_RUN_INTEGRITY_CHECKS=1`.
* Add ability to force DB integrity checks to run on node start by adding non-numeric contents to the initializer.pid file.
* Introducing fast-syncing to join the network, avoiding the need to execute every block to catch up.
* Added `archival_sync` to `[node]` config section, along with archival syncing capabilities
* Added `max_parallel_deploy_fetches` and `max_parallel_trie_fetches` config options to the `[node]` section to control how many requests are made in parallel while syncing.
* Add capabilities for known nodes to slow down the reconnection process of outdated legacy nodes still out on the internet.

### Changed
* Detection of a crash no longer triggers DB integrity checks to run on node start; the checks can be triggered manually instead.
* `SIGUSR1` now only dumps the queue in the debug text format.
* Major rewrite of the contract runtime component.
* More node modules are now `pub(crate)`.
* Chain automatically creates a switch block immediately after genesis or an upgrade.
* Asymmetric connections are now swept regularly again.
* Nodes no longer connect to nodes that do not speak the same protocol version by default.

### Deprecated
* Deprecate the `starting_state_root_hash` field from the REST and JSON-RPC status endpoints.

### Removed
* Legacy synchronization from genesis in favor of fast sync has been removed.



## [1.4.3] - 2021-12-06

### Added
* Add new event to the main SSE server stream accessed via `<IP:Port>/events/main` which emits hashes of expired deploys.

### Changed
* `enable_manual_sync` configuration parameter defaults to `true`.
* Default behavior of LMDB changed to use [`NO_READAHEAD`](https://docs.rs/lmdb/0.8.0/lmdb/struct.EnvironmentFlags.html#associatedconstant.NO_READAHEAD).
* Updated dependencies, in particular `casper-types` to use fixed checksummed-hex format.
* Add new event to the main SSE server stream across all endpoints `<IP:PORT>/events/*` which emits a shutdown event when the node shuts down.
* Add `SIGUSR2` signal handling to dump the queue in JSON format (see "Changed" section for `SIGUSR1`).



## [1.4.2] - 2021-11-11

### Changed
* There are now less false warnings/errors regarding dropped responders or closed channels during a shutdown, where they are expected and harmless.
* Execution transforms are ordered by insertion order.

### Removed
* The config option `consensus.highway.unit_hashes_folder` has been removed.

### Fixed
* The block proposer component now retains pending deploys and transfers across a restart.



## [1.4.0] - 2021-10-04

### Added
* Add `enable_manual_sync` boolean option to `[contract_runtime]` in the config.toml which enables manual LMDB sync.
* Add `contract_runtime_execute_block` histogram tracking execution time of a whole block.
* Long-running events now log their event type.
* Individual weights for traffic throttling can now be set through the configuration value `network.estimator_weights`.
* Add `consensus.highway.max_request_batch_size` configuration parameter. Defaults to 20.
* New histogram metrics `deploy_acceptor_accepted_deploy` and `deploy_acceptor_rejected_deploy` that track how long the initial verification took.
* Add gzip content negotiation (using accept-encoding header) to rpc endpoints.
* Add `state_get_trie` JSON-RPC endpoint.
* Add `info_get_validator_changes` JSON-RPC endpoint and REST endpoint `validator-changes` that return the status changes of active validators.

### Changed
* The following Highway timers are now separate, configurable, and optional (if the entry is not in the config, the timer is never called):
  * `standstill_timeout` causes the node to restart if no progress is made.
  * `request_state_interval` makes the node periodically request the latest state from a peer.
  * `log_synchronizer_interval` periodically logs the number of entries in the synchronizer queues.
* Add support for providing node uptime via the addition of an `uptime` parameter in the response to the `/status` endpoint and the `info_get_status` JSON-RPC.
* Support building and testing using stable Rust.
* Log chattiness in `debug` or lower levels has been reduced and performance at `info` or higher slightly improved.
* The following parameters in the `[gossip]` section of the config has been renamed:
  * `[finished_entry_duration_secs]` => `[finished_entry_duration]`
  * `[gossip_request_timeout_secs]` => `[gossip_request_timeout]`
  * `[get_remainder_timeout_secs]` => `[get_remainder_timeout]`
* The following parameters in config now follow the humantime convention ('30sec', '120min', etc.):
  * `[network][gossip_interval]`
  * `[gossip][finished_entry_duration]`
  * `[gossip][gossip_request_timeout]`
  * `[gossip][get_remainder_timeout]`
  * `[fetcher][get_from_peer_timeout]`

### Removed
* The unofficial support for nix-related derivations and support tooling has been removed.
* Experimental, nix-based kubernetes testing support has been removed.
* Experimental support for libp2p has been removed.
* The `isolation_reconnect_delay` configuration, which has been ignored since 1.3, has been removed.
* The libp2p-exclusive metrics of `read_futures_in_flight`, `read_futures_total`, `write_futures_in_flight`, `write_futures_total` have been removed.

### Fixed
* Resolve an issue where `Deploys` with payment amounts exceeding the block gas limit would not be rejected.
* Resolve issue of duplicated config option `max_associated_keys`.



## [1.3.2] - 2021-08-02

### Fixed
* Resolve an issue in the `state_get_dictionary_item` JSON-RPC when a `ContractHash` is used.
* Corrected network state engine to hold in blocked state for full 10 minutes when encountering out of order race condition.



## [1.3.1] - 2021-07-26

### Fixed
* Parametrized sync_timeout and increased value to stop possible post upgrade restart loop.



## [1.3.0] - 2021-07-19

### Added
* Add support for providing historical auction information via the addition of an optional block ID in the `state_get_auction_info` JSON-RPC.
* Exclude inactive validators from proposing blocks.
* Add validation of the `[protocol]` configuration on startup, to ensure the contained values make sense.
* Add optional outgoing bandwidth limiter to the networking component, controllable via new `[network][max_outgoing_byte_rate_non_validators]` config option.
* Add optional incoming message limiter to the networking component, controllable via new `[network][max_incoming_message_rate_non_validators]` config option.
* Add optional in-memory deduplication of deploys, controllable via new `[storage]` config options `[enable_mem_deduplication]` and `[mem_pool_prune_interval]`.
* Add a new event stream to SSE server accessed via `<IP:Port>/events/deploys` which emits deploys in full as they are accepted.
* Events now log their ancestors, so detailed tracing of events is possible.

### Changed
* Major rewrite of the network component, covering connection negotiation and management, periodic housekeeping and logging.
* Exchange and authenticate Validator public keys in network handshake between peers.
* Remove needless copying of outgoing network messages.
* Move finality signatures to separate event stream and change stream endpoints to `/events/main` and `/events/sigs`.
* Avoid truncating the state root hash when reporting node's status via JSON-RPC or REST servers.
* The JSON-RPC server waits until an incoming deploy has been sent to storage before responding to the client.
* Persist event stream event index across node restarts.
* Separate transfers from other deploys in the block proposer.
* Enable getting validators for future eras in `EffectBuilder::get_era_validators()`.
* Improve logging around stalled consensus detection.
* Skip storage integrity checks if the node didn't previously crash.
* Update pinned version of Rust to `nightly-2021-06-17`.
* Changed LMDB flags to reduce flushing and optimize IO performance in the Contract Runtime.
* Don't shut down by default anymore if stalled. To enable set config option `shutdown_on_standstill = true` in `[consensus.highway]`.
* Major rewrite of the contract runtime component.
* Ports used for local testing are now determined in a manner that hopefully leads to less accidental conflicts.
* At log level `DEBUG`, single events are no longer logged (use `TRACE` instead).
* More node modules are now `pub(crate)`.

### Removed
* Remove systemd notify support, including removal of `[network][systemd_support]` config option.
* Removed dead code revealed by making modules `pub(crate)`.
* The networking layer no longer gives preferences to validators from the previous era.

### Fixed
* Avoid redundant requests caused by the Highway synchronizer.
* Update "current era" metric also for initial era.
* Keep syncing until the node is in the current era, rather than allowing an acceptable drift.
* Update the list of peers with newly-learned ones in linear chain sync.
* Drain the joiner reactor queue on exit, to eliminate stale connections whose handshake has completed, but which live on the queue.
* Shut down SSE event streams gracefully.
* Limit the maximum number of clients connected to the event stream server via the `[event_stream_server][max_concurrent_subscribers]` config option.
* Avoid emitting duplicate events in the event stream.
* Change `BlockIdentifier` params in the Open-RPC schema to be optional.
* Asymmetric connections are now swept regularly again.



## [1.2.0] - 2021-05-27

### Added
* Add configuration options for `[consensus][highway][round_success_meter]`.
* Add `[protocol][last_emergency_restart]` field to the chainspec for use by fast sync.
* Add an endpoint at `/rpc-schema` to the REST server which returns the OpenRPC-compatible schema of the JSON-RPC API.
* Have consensus component shut down the node on an absence of messages for the last era for a given period.
* Add a new `Step` event to the event stream which displays the contract runtime `Step` execution results.
* Add a configurable delay before proposing dependencies, to give deploys time to be gossiped before inclusion in a new block.
* Add instrumentation to the network component.
* Add fetchers for block headers.
* Add joiner test.

### Changed
* Change to Apache 2.0 license.
* Provide an efficient way of finding the block to which a given deploy belongs.
* On hard-reset upgrades, only remove stored blocks with old protocol versions, and remove all data associated with a removed block.
* Restrict expensive startup integrity checks to sessions following unclean shutdowns.
* Improve node joining process.
* Improve linear chain component, including cleanups and optimized handling of finality signatures.
* Make the syncing process, linear chain component and joiner reactor not depend on the Era Supervisor.
* Improve logging of banned peers.
* Change trigger for upgrade checks to timed interval rather than announcement of new block.
* Use the same JSON representation for a block in the event stream as for the JSON-RPC server.
* Avoid creating a new era when shutting down for an upgrade.
* Allow consensus to disconnect from faulty peers.
* Use own most recent round exponent instead of the median when initializing a new era.
* Request protocol state from peers only for the latest era.
* Add an instance ID to consensus pings, so that they are only handled in the era and the network they were meant for.
* Avoid requesting a consensus dependency that is already in the synchronizer queue.
* Change replay detection to not use execution results.
* Initialize consensus round success meter with current timestamp.
* Era Supervisor now accounts for the last emergency restart.
* Upgrade dependencies, in particular tokio.

### Removed
* Remove `impl Sub<Timestamp> for Timestamp` to help avoid panicking in non-obvious edge cases.
* Remove `impl Sub<TimeDiff> for Timestamp` from production code to help avoid panicking in non-obvious edge cases.
* Remove `[event_stream_server][broadcast_channel_size]` from config.toml, and make it a factor of the event stream buffer size.

### Fixed
* Have casper-node process exit with the exit code returned by the validator reactor.
* Restore cached block proposer state correctly.
* Runtime memory estimator now registered in the joiner reactor.
* Avoid potential arithmetic overflow in consensus component.
* Avoid potential index out of bounds error in consensus component.
* Avoid panic on dropping an event responder.
* Validate each block size in the block validator component.
* Prevent deploy replays.
* Ensure finality signatures received after storing a block are gossiped and stored.
* Ensure isolated bootstrap nodes attempt to reconnect properly.
* Ensure the reactor doesn't skip fatal errors before successfully exiting.
* Collect only verified signatures from bonded validators.
* Fix a race condition where new metrics were replaced before the networking component had shut down completely, resulting in a panic.
* Ensure an era is not activated twice.
* Avoid redundant requests caused by the Highway synchronizer.
* Reduce duplication in block validation requests made by the Highway synchronizer.
* Request latest consensus state only if consensus has stalled locally.



## [1.1.1] - 2021-04-19

### Changed
* Ensure consistent validation when adding deploys and transfers while proposing and validating blocks.



## [1.1.0] - 2021-04-13 [YANKED]

### Changed
* Ensure that global state queries will only be permitted to recurse to a fixed maximum depth.



## [1.0.1] - 2021-04-08

### Added
* Add `[deploys][max_deploy_size]` to chainspec to limit the size of valid deploys.
* Add `[network][maximum_net_message_size]` to chainspec to limit the size of peer-to-peer messages.

### Changed
* Check deploy size does not exceed maximum permitted as part of deploy validation.
* Include protocol version and maximum message size in network handshake of nodes.
* Change accounts.toml to only be included in v1.0.0 configurations.



## [1.0.0] - 2021-03-30

### Added
* Initial release of node for Casper mainnet.



[Keep a Changelog]: https://keepachangelog.com/en/1.0.0
[unreleased]: https://github.com/casper-network/casper-node/compare/37d561634adf73dab40fffa7f1f1ee47e80bf8a1...dev
[1.4.2]: https://github.com/casper-network/casper-node/compare/v1.4.0...37d561634adf73dab40fffa7f1f1ee47e80bf8a1
[1.4.0]: https://github.com/casper-network/casper-node/compare/v1.3.0...v1.4.0
[1.3.0]: https://github.com/casper-network/casper-node/compare/v1.2.0...v1.3.0
[1.2.0]: https://github.com/casper-network/casper-node/compare/v1.1.1...v1.2.0
[1.1.1]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.1.0]: https://github.com/casper-network/casper-node/compare/v1.0.1...v1.1.1
[1.0.1]: https://github.com/casper-network/casper-node/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/casper-network/casper-node/releases/tag/v1.0.0
