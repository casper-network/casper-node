@0x8d8000b3ac0ebd80;

using import "digest.capnp".Digest;

using Rust = import "rust.capnp";
$Rust.parentModule("capnp::types::deploy_hash");

# Deploy hashes are Digest newtypes and have 32 bytes
struct DeployHash {
    digest @0 :Digest;
}