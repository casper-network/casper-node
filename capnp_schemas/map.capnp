@0x93f3291d9df65ce2;

struct Map(Key, Value) {
  entries @0 :List(Entry);
  struct Entry {
    key @0 :Key;
    value @1 :Value;
  }
}

# One of capnproto's design decisions is that generics must be pointers.
# Below are specialized maps with primitives as either their keys or their values.
# See: https://www.mail-archive.com/capnproto@googlegroups.com/msg01286.html

struct StringMap(Value) {
  entries @0 :List(Entry);
  struct Entry {
    key @0 :Text;
    value @1 :Value;
  }
}

struct WeightMap(Key) {
  entries @0 :List(Entry);
  struct Entry {
    key @0 :Key;
    value @1 :UInt8;
  }
}
