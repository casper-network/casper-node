@0xa4d471bd9486ce21;

using import "deploy_hash.capnp".DeployHash;
using import "public_key.capnp".PublicKey;

using Rust = import "rust.capnp";
$Rust.parentModule("capnp::types::block_body");

struct BlockBody {
    proposer @0 :PublicKey;
    deployHashes @1 :List(DeployHash);
    transferHashes @2 :List(DeployHash);
}