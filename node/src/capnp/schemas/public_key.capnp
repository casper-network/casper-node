@0x82282564c76d4c41;

using Rust = import "rust.capnp";
$Rust.parentModule("capnp::types::public_key");

# Ed25519 Public Keys have 32 bytes
struct Ed25519PublicKey {
    byte0 @0 :UInt8;
    byte1 @1 :UInt8;
    byte2 @2 :UInt8;
    byte3 @3 :UInt8;
    byte4 @4 :UInt8;
    byte5 @5 :UInt8;
    byte6 @6 :UInt8;
    byte7 @7 :UInt8;
    byte8 @8 :UInt8;
    byte9 @9 :UInt8;
    byte10 @10 :UInt8;
    byte11 @11 :UInt8;
    byte12 @12 :UInt8;
    byte13 @13 :UInt8;
    byte14 @14 :UInt8;
    byte15 @15 :UInt8;
    byte16 @16 :UInt8;
    byte17 @17 :UInt8;
    byte18 @18 :UInt8;
    byte19 @19 :UInt8;
    byte20 @20 :UInt8;
    byte21 @21 :UInt8;
    byte22 @22 :UInt8;
    byte23 @23 :UInt8;
    byte24 @24 :UInt8;
    byte25 @25 :UInt8;
    byte26 @26 :UInt8;
    byte27 @27 :UInt8;
    byte28 @28 :UInt8;
    byte29 @29 :UInt8;
    byte30 @30 :UInt8;
    byte31 @31 :UInt8;
}

# Compressed Secp256k1 Public Keys have 33 bytes
struct Secp256k1PublicKey {
     byte0 @0 :UInt8;
     byte1 @1 :UInt8;
     byte2 @2 :UInt8;
     byte3 @3 :UInt8;
     byte4 @4 :UInt8;
     byte5 @5 :UInt8;
     byte6 @6 :UInt8;
     byte7 @7 :UInt8;
     byte8 @8 :UInt8;
     byte9 @9 :UInt8;
     byte10 @10 :UInt8;
     byte11 @11 :UInt8;
     byte12 @12 :UInt8;
     byte13 @13 :UInt8;
     byte14 @14 :UInt8;
     byte15 @15 :UInt8;
     byte16 @16 :UInt8;
     byte17 @17 :UInt8;
     byte18 @18 :UInt8;
     byte19 @19 :UInt8;
     byte20 @20 :UInt8;
     byte21 @21 :UInt8;
     byte22 @22 :UInt8;
     byte23 @23 :UInt8;
     byte24 @24 :UInt8;
     byte25 @25 :UInt8;
     byte26 @26 :UInt8;
     byte27 @27 :UInt8;
     byte28 @28 :UInt8;
     byte29 @29 :UInt8;
     byte30 @30 :UInt8;
     byte31 @31 :UInt8;
     byte32 @32 :UInt8;
}

struct PublicKey {
    union {
        ed25519 @0 :Ed25519PublicKey;
        secp256k1 @1 :Secp256k1PublicKey;
        system @2 :Void;
    }
}
