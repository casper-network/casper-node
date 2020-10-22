import os
import subprocess
import json
import pathlib
import pickle
import time

from era_validators import parse_era_validators

# From:
# casper-client put-deploy --chain-name casper-charlie-testnet-7 --node-address http://localhost:7777 \
# --secret-key /etc/casper/validator_keys/secret_key.pem --session-path  ./wasm/system_contract_hashes.wasm \
# --payment-amount 10000000
#
# {"api_version":"1.0.0","deploy_hash":"af896f8e9ce9ace69c0f689193a2c4adb69eb40c1e4d1c2fb6582f0de7866eb0"}
#
# casper-client query-state --node-address http://localhost:7777 -k $(cat /etc/casper/validator_keys/public_key_hex) \
# -g $(casper-client get-global-state-hash --node-address http://localhost:7777 | jq -r '.["global_state_hash"]') \
# | jq -r '.["stored_value"]["Account"]["named_keys"]["auction"]'
#
# hash-c3dff9292e0d3a1076fe5ee60335c599195f88a62c9d1e50ac307f453e7fe0e2
AUCTION_HASH = 'hash-c3dff9292e0d3a1076fe5ee60335c599195f88a62c9d1e50ac307f453e7fe0e2'

NODE_ADDRESS = 'http://54.177.84.9:7777'

GET_GLOBAL_STATE_COMMAND = ["casper-client", "get-global-state-hash", "--node-address", NODE_ADDRESS]


def _subprocess_call(command, expect_text) -> dict:
    process = subprocess.Popen(command,
                               stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE)
    stdout, stderr = process.communicate(timeout=30)
    if expect_text.encode('utf-8') not in stdout:
        raise Exception(stderr.decode('utf-8'))
    return json.loads(stdout.decode('utf-8'))


def get_global_state_hash():
    response = _subprocess_call(GET_GLOBAL_STATE_COMMAND, "global_state_hash")
    return response["global_state_hash"]


def get_era_validators(global_state_hash):
    command = ["casper-client", "query-state",
               "--node-address", NODE_ADDRESS,
               "-k", AUCTION_HASH,
               "-g", global_state_hash,
               "-q", "era_validators"]
    response = _subprocess_call(command, "stored_value")
    era_validator_bytes = response["stored_value"]["CLValue"]["bytes"]
    return parse_era_validators(era_validator_bytes)


def get_block(block_hash=None):
    command = ["casper-client", "get-block",
               "--node-address", NODE_ADDRESS]
    if block_hash:
        command.append("-b")
        command.append(block_hash)
    return _subprocess_call(command, "block")


def get_all_blocks():
    """
    retrieves all blocks on chain and caches when possible

    will be REALLY slow with large block downloads as calls are throttled.
    """
    cached_blocks_file = pathlib.Path(os.path.realpath(__file__)).parent / "block_cache"
    if pathlib.Path.exists(cached_blocks_file):
        blocks = pickle.load(open(cached_blocks_file, "rb"))
        last_height = blocks[-1]["block"]["header"]["height"]
    else:
        blocks = []
        last_height = -1
    block = get_block()
    new_blocks = []
    cur_height = block["block"]["header"]["height"]
    for _ in range(cur_height - last_height):
        new_blocks.append(block)
        time.sleep(0.1)
        parent_hash = block["block"]["header"]["parent_hash"]
        if parent_hash != '0000000000000000000000000000000000000000000000000000000000000000':
            block = get_block(parent_hash)

    new_blocks.reverse()
    blocks.extend(new_blocks)
    pickle.dump(blocks, open(cached_blocks_file, "wb"))
    return blocks


current_global_state_hash = get_global_state_hash()
print(get_era_validators(current_global_state_hash))
# all_blocks = get_all_blocks()
#
# pre_gsh = ''
# for block in all_blocks:
#     header = block["block"]["header"]
#     gsh = header["global_state_hash"]
#     if pre_gsh != gsh:
#         pre_gsh = gsh
#         print(f"{header['era_id']} - {header['height']} - {gsh}")

# for block in all_blocks:
#     header = block["block"]["header"]
#     print(f'{header["era_id"]}, {header["height"]}')



# print(get_era_validators('6cee0f55fc73791f091578d47517b69e7ef18d0d5b1f051f9c4bd0b457a70e54'))