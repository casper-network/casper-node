const HEX_TABLE: String[] = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f'];

export function hex2bin(hex: String): Array<u8> {
  let bin = new Array<u8>(hex.length / 2);

  for (let i = 0; i < hex.length / 2; i++) {
    // NOTE: hex.substr + parseInt gives weird results under AssemblyScript
    const hi = HEX_TABLE.indexOf(hex[i * 2]);
    assert(hi > -1);
    const lo = HEX_TABLE.indexOf(hex[(i * 2) + 1]);
    assert(lo > -1);
    const number = (hi << 4) | lo;
    bin[i] = <u8>number;
  }
  return bin;
}
