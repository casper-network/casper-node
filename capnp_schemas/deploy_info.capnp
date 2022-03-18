@0xfdf107813ee821c4;

using import "hash_with_32_bytes.capnp".Hash;
using import "uref.capnp".URef;

struct DeployInfo {
  deployHash @0 :Hash;
  transfers @1 :List(Hash);
  from @2 :Hash;
  source @3 :URef;
  gas @4 :Data;
}