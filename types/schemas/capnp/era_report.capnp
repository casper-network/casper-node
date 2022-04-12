@0xbe9397eba3020229;

using import "public_key.capnp".PublicKey;
using import "map.capnp".RewardsMap;

struct EraReport {
    id @0 :List(PublicKey);
    rewards @1 :RewardsMap(PublicKey);
    inactiveValidators @2 :List(PublicKey);
}