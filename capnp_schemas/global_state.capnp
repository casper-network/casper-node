@0xf870a6924a6f4ac0;

using import "account.capnp".Account;
using import "deploy_info.capnp".DeployInfo;
using import "hash_with_32_bytes.capnp".Hash;
using import "transfer.capnp".Transfer;
using import "uref.capnp".URef;
using import "system_contract_registry.capnp".SystemContractRegistry;

struct AccountKey {
  prefixTag @0 :UInt8 = 0;
  accountHash @1 :Hash;
}

struct ContractKey {
  prefixTag @0 :UInt8 = 1;
  contractHash @1 :Hash;
}

struct Key {
  union {
    account @0 :Hash;
    hash @1 :Hash;
    uRef @2 :Hash;
    transfer @3 :Hash;
    deployInfo @4 :Hash;
    eraInfo @5 :UInt64;
    balance @6 :Hash;
    bid @7 :Hash;
    withdraw @8 :Hash;
    dictionary @9 :Hash;
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
    account @0 : Entry(Hash, Account);

    # TODO: These are wrong
    contract @1 :Entry(Hash, Hash);
    uref @2 :Entry(URef, Hash);

    # TODO: Consider moving these 3 out of global state & add them to the block body instead (or "block extras" or something)?
    transferAddr @3 :Entry(Hash, Transfer);
    deployInfo @4 :Entry(Hash, DeployInfo);
    # eraInfo @5 :Entry(EraId, );

    systemContractRegistry @5 :SystemContractRegistry;
  }
}