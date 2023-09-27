# Changelog

All notable changes to this project will be documented in this file.  The format is based on [Keep a Changelog].

[comment]: <> (Added:      new features)
[comment]: <> (Changed:    changes in existing functionality)
[comment]: <> (Deprecated: soon-to-be removed features)
[comment]: <> (Removed:    now removed features)
[comment]: <> (Fixed:      any bug fixes)
[comment]: <> (Security:   in case of vulnerabilities)



## Unreleased

### Added
* New environment variable `CL_EVENT_QUEUE_DUMP_THRESHOLD` to enable dumping of queue event counts to log when a certain threshold is exceeded.

### Fixed
* Now possible to build outside a git repository context (e.g. from a source tarball). In such cases, the node's build version (as reported vie status endpoints) will not contain a trailing git short hash.

### Changed
* The `state_identifier` parameter of the `query_global_state` JSON-RPC method is now optional. If no `state_identifier` is specified, the highest complete block known to the node will be used to fulfill the request.



## 1.5.2

### Added
* Added the `cors_origin` config option under the `[rest_server]`, `[rpc_server]`, `[event_stream_server]` and `[speculative_exec_server]` sections to allow configuration of the CORS Origin.



## 1.5.1

### Added
* Added the `upgrade_timeout` config option under the `[node]` section.

### Changed
* `speculative_exec` server now routes deploys to `DeployAcceptor` for more comprehensive validation, including cryptographic verification of signatures.



## 1.5.0-rc.1

### Added
* Introduce fast-syncing to join the network, avoiding the need to execute every block to catch up.
* Add config sections for new components to support fast-sync: `[block_accumulator]`, `[block_synchronizer]`, `[deploy_buffer]` and `[upgrade_watcher]`.
* Add new Zug consensus protocol, disabled by default, along with a new `[consensus.zug]` config section.
* Add a `consensus_protocol` option to the chainspec to choose a consensus protocol, and a `minimum_block_time` setting for the minimum difference between a block's timestamp and its child's.
* Add a `vesting_schedule_period` option to the chainspec to define the period in which genesis validators' bids are released over time after they are unlocked.
* Add a `simultaneous_peer_requests` option to the chainspec to define the maximum number of simultaneous block-sync and sync-leap requests.
* Add following config options under `[node]` section to support fast-sync:
  * `sync_to_genesis` which if set to `true` will cause the node to retrieve all blocks, deploys and global state back to genesis.
  * `idle_tolerance` which defines the time after which the syncing process is considered stalled.
  * `max_attempts` which defines the maximum number of attempts to sync before exiting the node process after the syncing process is considered stalled.
  * `control_logic_default_delay` which defines the default delay for the control events that have no dedicated delay requirements.
  * `force_resync` which if set to `true` will cause the node to resync all of the blocks.
* Add following config options under `[network]` section:
  * `min_peers_for_initialization` which defines the minimum number of fully-connected peers to consider network component initialized.
  * `handshake_timeout` which defines connection handshake timeouts (they were hardcoded at 20 seconds previously).
  * `max_incoming_peer_connections` which defines the maximum number of incoming connections per unique peer allowed.
  * `max_in_flight_demands` which defines the maximum number of in-flight requests for data from a single peer.
  * `tarpit_version_threshold`, `tarpit_duration` and `tarpit_chance` to configure the tarpitting feature, designed to reduce the impact of old node versions making repeated, rapid reconnection attempts.
  * `blocklist_retain_duration` which defines how long peers remain blocked after they get blocklisted.
  * optional `[network.identity]` section to support loading existing network identity certificates signed by a certificate authority.
  * In addition to `consensus` and `deploy_requests`, the following values can now be controlled via the `[network.estimator_weights]` section in config: `gossip`, `finality_signatures`, `deploy_responses`, `block_requests`, `block_responses`, `trie_requests` and `trie_responses`.
* The network handshake now contains the hash of the chainspec used and will be successful only if they match.
* Checksums for execution results and deploy approvals are written to global state after each block execution.
* Add a new config option `[rpc_server.max_body_bytes]` to allow a configurable value for the maximum size of the body of a JSON-RPC request.
* Add `enable_server` option to all HTTP server configuration sections (`rpc_server`, `rest_server`, `event_stream_server`) which allow users to enable/disable each server independently (enabled by default).
* Add `enable_server`, `address`, `qps_limit` and `max_body_bytes` to new `speculative_exec_server` section to `config.toml` to configure speculative execution JSON-RPC server (disabled by default).
* Add new event to the main SSE server stream across all endpoints `<IP:PORT>/events/*` which emits a shutdown event when the node shuts down.
* Add following fields to the `/status` endpoint and the `info_get_status` JSON-RPC:
  * `reactor_state` indicating the node's current operating mode.
  * `last_progress` indicating the time the node last made progress.
  * `available_block_range` indicating the highest contiguous sequence of the block chain for which the node has complete data.
  * `block_sync` indicating the state of the block synchronizer component.
* Add new REST `/chainspec` and JSON-RPC `info_get_chainspec` endpoints that return the raw bytes of the `chainspec.toml`, `accounts.toml` and `global_state.toml` files as read at node startup.
* Add a new JSON-RPC endpoint `query_balance` which queries for balances under a given `PurseIdentifier`.
* Add new JSON-RPC endpoint `/speculative_exec` that accepts a deploy and a block hash and executes that deploy, returning the execution effects.
* Add `strict_argument_checking` to the chainspec to enable strict args checking when executing a contract; i.e. that all non-optional args are provided and of the correct `CLType`.
* A diagnostics port can now be enabled via the `[diagnostics_port]` config section. See the `README.md` for details.
* Add `SIGUSR2` signal handling to dump the queue in JSON format (see "Changed" section for `SIGUSR1`).
* Add `validate_and_store_timeout` config option under `[gossip]` section to control the time the gossiper waits for another component to validate and store an item received via gossip.
* Add metrics:
  * `block_accumulator_block_acceptors`, `block_accumulator_known_child_blocks` to report status of the block accumulator component
  * `(forward|historical)_block_sync_duration_seconds` to report the progress of block synchronization
  * `deploy_buffer_total_deploys`, `deploy_buffer_held_deploys`, `deploy_buffer_dead_deploys` to report status of the deploy buffer component
  * `(lowest|highest)_available_block_height` to report the low/high values of the complete block range (the highest contiguous chain of blocks for which the node has complete data)
  * `sync_leap_duration_seconds`, `sync_leap_fetched_from_peer_total`, `sync_leap_rejected_by_peer_total`, `sync_leap_cant_fetch_total` to report progress of the sync leaper component
  * `execution_queue_size` to report the number of blocks enqueued pending execution
  * `accumulated_(outgoing|incoming)_limiter_delay` to report how much time was spent throttling other peers.
* Add `testing` feature to casper-node crate to support test-only functionality (random constructors) on blocks and deploys.
* Connections to unresponsive nodes will be terminated, based on a watchdog feature.

### Changed
* The `starting_state_root_hash` field from the REST and JSON-RPC status endpoints now represents the state root hash of the lowest block in the available block range.
* Detection of a crash no longer triggers DB integrity checks to run on node start; the checks can be triggered manually instead.
* Nodes no longer connect to nodes that do not speak the same protocol version by default.
* Incoming connections from peers are rejected if they are exceeding the default incoming connections per peer limit of 3.
* Chain automatically creates a switch block immediately after genesis or an upgrade, known as "immediate switch blocks".
* Requests for data from a peer are now de-prioritized over networking messages necessary for consensus and chain advancement.
* Replace network message format with a more efficient encoding while keeping the initial handshake intact.
* Flush outgoing messages immediately, trading bandwidth for latency and hence optimizing feedback loops of various components in the system.
* Move `finality_threshold_fraction` from the `[highway]` to the `[core]` section in the chainspec.
* Move `max_execution_delay` config option from `[consensus.highway]` to `[consensus]` section.
* Add CORS behavior to allow any route on the JSON-RPC, REST and SSE servers.
* The JSON-RPC server now returns more useful responses in many error cases.
* Add a new parameter to `info_get_deploys` JSON-RPC, `finalized_approvals` - controlling whether the approvals returned with the deploy should be the ones originally received by the node, or overridden by the approvals that were finalized along with the deploy.
* Support using block height as the `state_identifier` parameter of JSON-RPC `query_global_state` requests.
* Add new `block_hash` and `block_height` optional fields to JSON-RPC `info_get_deploy` response which will be present when execution results aren't available.
* JSON-RPC responses which fail to provide requested data will now also include an indication of that node's available block range, i.e. the block heights for which it holds all global state.  See [#2789](https://github.com/casper-network/casper-node/pull/2789) for an example of the new error response.
* Add a `lock_status` field to the JSON representation of the `ContractPackage` values.
* `Key::SystemContractRegistry` is now readable and can be queried via the `query_global_state` JSON-RPC.
* Unify log messages for blocked nodes and provide more detailed reasons for blocking peers.
* Rename `current_era` metric to `consensus_current_era`.

### Deprecated
* `null` should no longer be used as a value for `params` in JSON-RPC requests.  Prefer an empty Array or Object.
* Deprecate the `chain_height` metric in favor of `highest_available_block_height`.

### Removed
* Remove legacy synchronization from genesis in favor of fast-sync.
* Remove config options no longer required due to fast-sync: `[linear_chain_sync]`, `[block_proposer]` and `[consensus.highway.standstill_timeout]`.
* Remove chainspec setting `[protocol.last_emergency_restart]` as fast sync will use the global state directly for recognizing such restarts instead.
* Remove a temporary chainspec setting `[core.max_stored_value_size]` which was used to limit the size of individual values stored in global state.
* Remove config section `[deploy_acceptor]` which only has one option `verify_accounts`, meaning deploys received from clients always undergo account balance checks to assess suitability for execution or not.
* Remove storage integrity check.
* Remove `SIGUSR1`/`SIGUSR2` queue dumps in favor of the diagnostics port.
* Remove `casper-mainnet` feature flag.

### Fixed
* Limiters for incoming requests and outgoing bandwidth will no longer inadvertently delay some validator traffic when maxed out due to joining nodes.
* Dropped connections no longer cause the outstanding messages metric to become incorrect.
* JSON-RPC server is now mostly compliant with the standard. Specifically, correct error values are now returned in responses in many failure cases.

### Security
* Bump `openssl` crate to version 0.10.48, if compiling with vendored OpenSSL to address latest RUSTSEC advisories.



## 1.4.15-alt

### Changed
* Update dependencies (in particular `casper-types` to v2.0.0 due to additional `Key` variant).  Note that publishing `1.4.15-alt` is only to rectify the issue where `casper-types` was published as v1.6.0 despite having a breaking change.  It is expected to only be consumed as a crate; there will be no upgrade of Casper Mainnet, Testnet, etc to protocol version `1.4.15-alt`.



## 1.4.15

### Changed
* Modified JSON-RPCs `chain_get_era_info_by_switch_block` and `chain_get_era_summary` to use either `Key::EraInfo` or `Key::EraSummary` as appropriate in order to provide useful responses.



## 1.4.14

### Added
* Node executes new prune process after executing each block, whereby entries under `Key::EraInfo` are removed in batches of size defined by the new chainspec option `[core.prune_batch_size]`.
* After executing a switch block, information about that era is stored to global state under a new static key `Key::EraSummary`.
* Add a new JSON-RPC endpoint `chain_get_era_summary` to retrieve the information stored under `Key::EraSummary`.

### Changed
* Rather than storing an ever-increasing collection of era information after executing a switch block under `Key::EraInfo`, the node now stores only the information relevant to that era under `Key::EraSummary`.
* Update `openssl` and `openssl-sys` to latest versions.

### Removed
* Remove asymmetric key functionality (move to `casper-types` crate behind feature `std`).
* Remove time types (move to `casper-types` with some functionality behind feature `std`).

### Fixed
* Fix issue in BlockValidator inhibiting the use of fallback peers to fetch missing deploys.



## 1.4.13

### Changed
* Update `casper-execution-engine`.



## 1.4.8

### Added
* Add an `identity` option to load existing network identity certificates signed by a CA.



### Changed
* Update `casper-execution-engine`.



## 1.4.7

### Changed
* Update `casper-execution-engine` and three `openssl` crates to latest versions.



## 1.4.6

### Changed
* Update dependencies to make use of scratch global state in the contract runtime.



## 1.4.5

### Added
* Add a temporary chainspec setting `max_stored_value_size` to limit the size of individual values stored in global state.
* Add a chainspec setting `minimum_delegation_amount` to limit the minimal amount of motes that can be delegated by a first time delegator.
* Add a chainspec setting `block_max_approval_count` to limit the maximum number of approvals across all deploys in a single block.
* Add a `finalized_approvals` field to the GetDeploy RPC, which if `true` causes the response to include finalized approvals substituted for the originally-received ones.

### Fixed
* Include deploy approvals in block payloads upon which consensus operates.
* Fixes a bug where historical auction data was unavailable via `get-auction-info` RPC.



## 1.4.4 - 2021-12-29

### Added
* Add `contract_runtime_latest_commit_step` gauge metric indicating the execution duration of the latest `commit_step` call.

### Changed
* No longer checksum-hex encode various types.



## 1.4.3 - 2021-12-06

### Added
* Add new event to the main SSE server stream accessed via `<IP:Port>/events/main` which emits hashes of expired deploys.

### Changed
* `enable_manual_sync` configuration parameter defaults to `true`.
* Default behavior of LMDB changed to use [`NO_READAHEAD`](https://docs.rs/lmdb/0.8.0/lmdb/struct.EnvironmentFlags.html#associatedconstant.NO_READAHEAD).



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
* Use `minimum_block_time` and `maximum_round_length` in Highway, instead of `minimum_round_exponent` and `maximum_round_exponent`. The minimum round length doesn't have to be a power of two in milliseconds anymore.

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
