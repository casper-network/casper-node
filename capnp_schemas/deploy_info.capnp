@0xfdf107813ee821c4;

using import "hash_with_32_bytes.capnp".Hash32;
using import "uref.capnp".URef;

struct DeployInfo {
  deployHash32 @0 :Hash32;
  transfers @1 :List(Hash32);
  from @2 :Hash32;
  source @3 :URef;
  gas @4 :Data;
}