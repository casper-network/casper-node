import struct

# era_validators bytes example, note no linefeeds or spaces in actual data
# 04000000 <- number of eras?
# 6b00000000000000   <- era id
# 0f000000           <- validator count
# 011117189c666f81c5160cd610ee383dc9b2d0361f004934754d39752eedc64957 0344420f  (key, bond amt)
# 0123bf1bd107d82bb42f4a92fc1ce19251f341802e1ecfa6ed88a3c7bbbdc302d1 0349420f
# 01313a3192db07c50758138e9637faf0358081c2c8709f8a72372685baeaf2a8b0 0347420f
# 0144118517b5ba7e7ca917d5a418c5fb8bf49e279427435d6597f59f8c5bf9ff1f 0346420f
# 015ff157d012383e99a3ae93bed0586a23383eed41b0058e63f48ed7b82b067c34 0341420f
# 016af0262f67aa93a225d9d57451023416e62aaa8391be8e1c09b8adbdef9ac19d 0390910f
# 017e2eafb31dac86a806f4da908151ffd2b4d552eced51731c241c94997ac310df 034e420f
# 0188ea4749619a58f7d40f05b3512eddbf48354b440f9154f84417115e3a7b188f 034a420f
# 0189f0413ecb256525d17bd8e4f194cdf063501b959716410de2216c1c24e1f0ce 034d420f
# 01a89e3f551efa337053592a0beb5fe50be58e12fc56f3c0eabf9344956a44a290 034c420f
# 01aaa7deb9ec99d6bed089fc938cac4a77bc9539f53556af057f9351a1d3c81de9 0345420f
# 01ae2af99944b0b9f92fccf425aa51894ebbad0f4e8e42c66a71dcb999a3bd94ed 0343420f
# 01bdda22721033ae556e92ccf1034f8d14f24c3794fa7befaa831bcae98f9f1e9b 0342420f
# 01c3e4895db6460c68d30076bd4391fc72fe2150d6ec77ec716f51ea2687b9f7bd 0348420f
# 01cb4be11fae27723ac9e88dff27331b836f15ff8227ac335d8dfc7602c603badc 0340420f
# 6c00000000000000
# 0f000000
# 011117189c666f81c5160cd610ee383dc9b2d0361f004934754d39752eedc64957 0344420f
# ...
# 6d00000000000000
# 0f000000
# 011117189c666f81c5160cd610ee383dc9b2d0361f004934754d39752eedc64957 0344420f
# ...
# 6e00000000000000
# 0f000000
# 011117189c666f81c5160cd610ee383dc9b2d0361f004934754d39752eedc64957 0344420f
# ...
# 01cb4be11fae27723ac9e88dff27331b836f15ff8227ac335d8dfc7602c603badc 0340420f"
import time

PREFIX = "04000000"
ERA_ID_LEN = 16
VALIDATOR_COUNT_LEN = 8
KEY_LEN = 66


def _era_to_decimal(era_hex) -> int:
    assert len(era_hex) == 16
    return struct.unpack('<Q', bytes.fromhex(era_hex))[0]


def _decimal_to_era(value) -> str:
    return struct.pack('<Q', value).hex()


def _validator_count_to_decimal(hex_str) -> int:
    assert len(hex_str) == 8
    return struct.unpack('<L', bytes.fromhex(hex_str))[0]


def _get_era(data, cur_pos) -> (int, int):
    """ returns (next_pos, era) """
    era = _era_to_decimal(data[cur_pos:cur_pos + ERA_ID_LEN])
    return cur_pos + ERA_ID_LEN, era


def _get_validator_count(data, cur_pos) -> (int, int):
    """ returns (next_pos, validator_count) """
    validator_count = _validator_count_to_decimal(data[cur_pos:cur_pos + VALIDATOR_COUNT_LEN])
    return cur_pos + VALIDATOR_COUNT_LEN, validator_count


def _get_u512_data(data, cur_pos) -> (int, int):
    """ return (next_pos, bond_amount) """
    u512_len = int(data[cur_pos:cur_pos+2], 16)
    cur_pos += 2
    raw_u512_hex = data[cur_pos:cur_pos+2*u512_len]
    padded_u512 = raw_u512_hex + '0000000000000000'[len(raw_u512_hex):]
    cur_pos += 2*u512_len
    bond_value = struct.unpack('<Q', bytes.fromhex(padded_u512))[0]
    return cur_pos, bond_value


def _get_validator_data(data, cur_pos) -> (int, list):
    """ return (next_pos, (validator_key, bond_amount)) """
    key_lens = {'01': 66}
    length = key_lens[data[cur_pos:cur_pos+2]]
    key = data[cur_pos:cur_pos+length]
    cur_pos += length
    cur_pos, bond_value = _get_u512_data(data, cur_pos)
    return cur_pos, (key, bond_value)


def _get_validators(data, cur_pos, validator_count) -> (int, list):
    """ return (next_pos, validators_list) """
    validators = []
    for _ in range(validator_count):
        cur_pos, validator_data = _get_validator_data(data, cur_pos)
        validators.append(validator_data)
    return cur_pos, validators


def _parse_era(data, cur_pos) -> (int, list):
    """ returns (next_pos, list of data) """
    cur_pos, era = _get_era(data, cur_pos)
    cur_pos, validator_count = _get_validator_count(data, cur_pos)
    cur_pos, validators = _get_validators(data, cur_pos, validator_count)
    return cur_pos, (era, validator_count, validators)


def parse_era_validators(data) -> list:
    """ parses full bytes output in hex from era_validators """
    assert data[:8] == PREFIX
    end_pos = len(data)
    cur_pos = 8
    full_data = []
    while cur_pos < end_pos:
        cur_pos, era_data = _parse_era(data, cur_pos)
        full_data.append(era_data)
    return full_data
