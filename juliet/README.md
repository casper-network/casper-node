# Juliet protocol implementation

This crate implements the Juliet multiplexing protocol as laid out in the [Juliet RFC](https://github.com/marc-casperlabs/juliet-rfc/blob/master/juliet.md). It aims to be a secure, simple, easy to verify/review implementation that is still reasonably performant.

## Benefits

 The Juliet protocol comes with a core set of features, such as

* carefully designed with security and DoS resilience as its foremoast goal,
* customizable frame sizes,
* up to 256 multiplexed, interleaved channels,
* backpressure support fully baked in, and
* low overhead (4 bytes per frame + 1-5 bytes depending on payload length).

This crate's implementation includes benefits such as

* a side-effect-free implementation of the Juliet protocol,
* an `async` IO layer integrated with the [`bytes`](https://docs.rs/bytes) crate to use it, and
* a type-safe RPC layer built on top.

## Examples

For a quick usage example, see `examples/fizzbuzz.rs`.

## `tracing` support

The crate has an optional dependency on the [`tracing`](https://docs.rs/tracing) crate, which, if enabled, allows detailed insights through logs. If the feature is not enabled, no log statements are compiled in.

Log levels in general are used as follows:

* `ERROR` and `WARN`: Actual issues that are not protocol level errors -- peer errors are expected and do not warrant a `WARN` level.
* `INFO`: Insights into received high level events (e.g. connection, disconnection, etc), except information concerning individual requests/messages.
* `DEBUG`: Detailed insights down to the level of individual requests, but not frames. A multi-megabyte single message transmission will NOT clog the logs.
* `TRACE`: Like `DEBUG`, but also including frame and wire-level information, as well as local functions being called.

At `INFO`, it is thus conceivable for a peer to maliciously spam local logs, although with some effort if connection attempts are rate limited. At `DEBUG` or lower, this becomes trivial.
