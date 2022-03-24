@0xa8492fb5524cebf8;

using import "global_state.capnp".Key;
using import "hash_with_32_bytes.capnp".Hash32;
using import "map.capnp".StringMap;
using import "map.capnp".WeightMap;
using import "uref.capnp".URef;

struct Account {
  # TODO: What are the restrictions on namedKeys?
  namedKeys @0 :StringMap(Key);
  mainPurse @1 :URef;
  associatedKeys @2 :WeightMap(Hash32);
  deploymentExecutionActionThreshold @3 :UInt8;
  keyManagementActionThreshold @4 :UInt8;
}