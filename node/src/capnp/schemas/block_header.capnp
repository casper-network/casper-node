@0xe997c10ed8dcfdcc;

using import "digest.capnp".Digest;
using import "era.capnp".EraEnd;
using import "era.capnp".EraId;

using Rust = import "rust.capnp";
$Rust.parentModule("capnp::types::block_header");

struct BlockHeader {
    parentHash @0 :BlockHash;
    stateRootHash @1 :Digest;
    bodyHash @2 :Digest;
    randomBit @3 :Bool;
    accumulatedSeed @4 :Digest;
    eraEnd @5 :MaybeEraEnd;
    timestamp @6 :Timestamp;
    eraId @7 :EraId;
    height @8 :UInt64;
    protocolVersion @9 :ProtocolVersion;
}

struct BlockHash {
    hash @0 :Digest;
}

struct MaybeEraEnd {
    union {
        someEraEnd @0 :EraEnd;
        none @1 :Void;
    }
}

struct SemVer {
    major @0 :UInt32;
    minor @1 :UInt32;
    patch @2 :UInt32;
}

struct ProtocolVersion {
    semver @0 :SemVer;
}

struct Timestamp {
    inner @0 :UInt64;
}