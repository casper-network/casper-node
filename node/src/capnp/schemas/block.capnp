@0x89377f5fe1e873d4;

using import "block_body.capnp".BlockBody;
using import "block_header.capnp".BlockHash;
using import "block_header.capnp".BlockHeader;

using Rust = import "rust.capnp";
$Rust.parentModule("capnp::types::block");

struct Block {
    hash @0 :BlockHash;
    header @1 :BlockHeader;
    body @2 :BlockBody;
}
