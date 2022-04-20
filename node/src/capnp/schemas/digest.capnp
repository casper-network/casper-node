@0xb17b764f35959c0b;

using Rust = import "rust.capnp";
$Rust.parentModule("capnp::types::digest");

# Digest is a newtype around 32 bytes.
struct Digest {
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