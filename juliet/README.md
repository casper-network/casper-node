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
