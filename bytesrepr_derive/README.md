# `bytesrepr_derive`

The `bytesrepr_derive` crate allows for deriving an implementation of the `casper_types::bytesrepr::{ToBytes/FromBytes}` trait by using the respective `ToBytes`/`FromBytes` macro. Example:

```rust
#[derive(FromBytes, ToBytes)]
struct MyStruct {
    a: u64,
    b: String,
}
```

To inspect the generated code, set the environment variable `BYTESREPR_DERIVE_DEBUG` to `1`, which cause it to be output to `stderr` during compilation.
