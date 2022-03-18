@0xbcae94a0a86b782f;

# TODO: Delete me?

struct CLResultType {
  union {
    ok @0: CLValue;
    err @1: CLValue;
  }
}

struct CLMapEntryType {
  key @0: CLValue;
  err @1: CLValue;
}

struct CLTuple1Type {
  first @0: CLValue;
}

struct CLTuple2Type {
  first @0: CLValue;
  second @1: CLValue;
}

struct CLTuple3Type {
  first @0: CLValue;
  second @1: CLValue;
  third @2: CLValue;
}

struct CLValue {
  union {
    bool @0: Bool;
    i32 @1: Int32;
    i64 @2: Int64;
    u8 @3: UInt8;
    u32 @4: UInt32;
    u64 @5: UInt64;
    u128 @6: Data;
    u256 @7: Data;
    u512 @8: Data;
    unit @9: Void;
    string @10: Text;
    key @11: import "global_state.capnp".Key;
    # TODO: Why do we need this if there's already a key variant that's a URef?
    uRef @12: import "uref.capnp".URef;

    publicKey @13: import "public_key.capnp".PlatformPublicKey;

    # TODO: We could use a variant here, or just use that pointers are always optional in capnp
    option @14: CLValue;

    list @15: List(CLValue);
    byteArray @16: Data;

    # TODO: Just get rid of this?
    result @17: CLResultType;

    # TODO: Get rid of this since it leads to contracts deep-copying?
    map @18: import "map.capnp".Map(CLValue, CLValue);

    # TODO: Get rid of these?
    tuple1 @19: CLTuple1Type;
    tuple2 @20: CLTuple2Type;
    tuple3 @21: CLTuple3Type;

    # TODO: Drop this and just use byteArray?
    any @22: Data;
  }
}