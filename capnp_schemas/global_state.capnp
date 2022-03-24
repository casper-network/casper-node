@0xf870a6924a6f4ac0;

using import "account.capnp".Account;
using import "deploy_info.capnp".DeployInfo;
using import "hash_with_32_bytes.capnp".Hash32;
using import "map.capnp".StringMap;
using import "transfer.capnp".Transfer;
using import "uref.capnp".URef;

struct Key {
  union {
    account @0 :Hash32;
    hash @1 :Hash32;
    uRef @2 :Hash32;
    transfer @3 :Hash32;
    deployInfo @4 :Hash32;
    eraInfo @5 :UInt64;
    balance @6 :Hash32;
    bid @7 :Hash32;
    withdraw @8 :Hash32;
    dictionary @9 :Hash32;
    systemContractRegistry @10 :Void;
  }
}

struct GlobalStateEntry {

  struct Entry(K,V) {
    key @0 :K;
    value @1 :V;
  }

  struct EraId {
    eraId @0 :UInt8;
  }

  union {
    account @0 :Entry(Hash32, Account);

    # TODO: These are wrong
    contract @1 :Entry(Hash32, Hash32);
    uref @2 :Entry(URef, Hash32);

    # TODO: Consider moving these 3 out of global state & add them to the block body instead (or "block extras" or something)?
    transferAddr @3 :Entry(Hash32, Transfer);
    deployInfo @4 :Entry(Hash32, DeployInfo);
    # eraInfo @5 :Entry(EraId, );

    systemContractRegistry @5 :StringMap(Hash32);
  }
}
