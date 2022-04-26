@0xddfeb5fd94f09c22;

using import "weight.capnp".Weight;
using import "account_hash.capnp".AccountHash;

using Rust = import "rust.capnp";
$Rust.parentModule("capnp::types::associated_keys");

struct AssociatedKeys {
    value @0 :AssociatedKeysMap(AccountHash);
}

struct AssociatedKeysMap(Key) {
  entries @0 :List(Entry);
  struct Entry {
    key @0 :Key;
    value @1 :Weight;
  }
}
