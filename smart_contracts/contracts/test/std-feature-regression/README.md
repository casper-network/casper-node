This contract exists purely to ensure the `std` feature of the `casper-contract` crate continues to be usable in a
backwards compatible manner.

Compiling the contract successfully is enough; it doesn't need to be run.

The contract specifies `#![no_std]` but by enabling the `casper-contract/std` feature, that should link the std lib,
providing the std global allocator.
