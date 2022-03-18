@0xda0524663a12929b;

using import "hash_with_32_bytes.capnp".Hash;

struct SystemContractRegistry {
  mint @0 :Hash;
  handlePayment @1 :Hash;
  standardPayment @2 :Hash;
  auction @3 :Hash;
}