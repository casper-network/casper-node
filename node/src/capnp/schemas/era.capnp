@0xb43fd725a9158445;

using import "common.capnp".U512;
using import "public_key.capnp".PublicKey;

using Rust = import "rust.capnp";
$Rust.parentModule("capnp::types::era");

struct EraId {
    id @0 :UInt64;
}

struct RewardsMap(Key) {
  entries @0 :List(Entry);
  struct Entry {
    key @0 :Key;
    value @1 :UInt64;
  }
}

struct EraReport {
  equivocators @0 :List(PublicKey);
  rewards @1 :RewardsMap(PublicKey);
  inactiveValidators @2 :List(PublicKey);
}

struct WeightsMap(Key) {
  entries @0 :List(Entry);
  struct Entry {
    key @0 :Key;
    value @1 :U512;
  }
}

struct EraEnd {
  eraReport @0 :EraReport;
  nextEraValidatorWeights @1 :WeightsMap(PublicKey);
}
