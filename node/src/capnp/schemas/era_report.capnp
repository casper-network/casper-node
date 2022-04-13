@0xbe9397eba3020229;

using import "public_key.capnp".PublicKey;
using import "map.capnp".RewardsMap;

using Rust = import "rust.capnp";
$Rust.parentModule("capnp::types::era_report");

struct EraReport {
    equivocators @0 :List(PublicKey);
    rewards @1 :RewardsMap(PublicKey);
    inactiveValidators @2 :List(PublicKey);
}