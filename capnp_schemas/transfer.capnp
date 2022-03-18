@0xb9a7130ecbd1c7ba;

using import "hash_with_32_bytes.capnp".Hash;
using import "uref.capnp".URef;

struct Transfer {
  deployHash @0 :Hash;
  from @1 :Hash;
  to @2 :Destination;

  # TODO: Why do these need to be URefs and not just hashes?
  source @3 :URef;
  target @4 :URef;

  amount @5 :Data;
  gas @6 :Data;
  id @7 :UserDefinedId;

  struct Destination {
    union {
       anotherAccountHash @0 :Hash;
       notApplicable @1 :Void;
    }
  }

  struct UserDefinedId {
    union {
      id @0: UInt8;
      notSpecified @1: Void;
    }
  }
}