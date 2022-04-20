@0x9e126c05ee94717e;

using Rust = import "rust.capnp";
$Rust.parentModule("capnp::types::common");

struct Option(T) {
    union {
        some @0 :T;
        none @1 :Void;
    }
}

struct Map(Key, Value) {
  entries @0 :List(Entry);
  struct Entry {
    key @0 :Key;
    value @1 :Value;
  }
}

struct U512 {
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
  byte33 @33 :UInt8;
  byte34 @34 :UInt8;
  byte35 @35 :UInt8;
  byte36 @36 :UInt8;
  byte37 @37 :UInt8;
  byte38 @38 :UInt8;
  byte39 @39 :UInt8;
  byte40 @40 :UInt8;
  byte41 @41 :UInt8;
  byte42 @42 :UInt8;
  byte43 @43 :UInt8;
  byte44 @44 :UInt8;
  byte45 @45 :UInt8;
  byte46 @46 :UInt8;
  byte47 @47 :UInt8;
  byte48 @48 :UInt8;
  byte49 @49 :UInt8;
  byte50 @50 :UInt8;
  byte51 @51 :UInt8;
  byte52 @52 :UInt8;
  byte53 @53 :UInt8;
  byte54 @54 :UInt8;
  byte55 @55 :UInt8;
  byte56 @56 :UInt8;
  byte57 @57 :UInt8;
  byte58 @58 :UInt8;
  byte59 @59 :UInt8;
  byte60 @60 :UInt8;
  byte61 @61 :UInt8;
  byte62 @62 :UInt8;
  byte63 @63 :UInt8;
}