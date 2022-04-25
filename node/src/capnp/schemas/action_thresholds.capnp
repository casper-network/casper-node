@0xc556cca47d47154f;

using import "weight.capnp".Weight;

using Rust = import "rust.capnp";
$Rust.parentModule("capnp::types::action_thresholds");

struct ActionThresholds {
    deployment @0 :Weight;
    keyManagement @1 :Weight;
}