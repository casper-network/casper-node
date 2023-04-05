#!/usr/bin/env python3

import datetime
import os
import random
import re
import subprocess
import sys
import threading
from time import sleep

# How long to keep the test running (assuming errorless run)
TEST_DURATION_SECS = 30 * 60

# Wasm transfers
DEPLOY_SPAM_INTERVAL_SECS = 3 * 60
DEPLOY_SPAM_COUNT = 600

# Named keys bloat
HUGE_DEPLOY_SPAM_INTERVAL_SECS = 60
HUGE_DEPLOY_SPAM_COUNT = 2

# How long to wait between invoking disturbance scenarios
DISTURBANCE_INTERVAL_SECS = 5

# Time allowed for the network to progress
PROGRESS_WAIT_TIMEOUT_SECS = 2 * 60

invoke_lock = threading.Lock()
current_node_count = 5
path_to_client = ""
huge_deploy_path = "./utils/nctl/sh/scenarios/smart_contracts/named_keys_bloat.wasm"


# Kill a random node, wait one minute, restart node
def disturbance_1(node_count):
    log("*** starting disturbance type 1 ***")
    random_node = random.randint(1, node_count)
    stop_node(random_node)
    sleep(60)
    start_node(random_node)
    return


# Kill two random nodes, wait one minute, restart nodes
def disturbance_2(node_count):
    log("*** starting disturbance type 2 ***")
    random_nodes = random.sample(range(1, node_count), 2)
    stop_node(random_nodes[0])
    stop_node(random_nodes[1])
    sleep(60)
    start_node(random_nodes[0])
    start_node(random_nodes[1])
    return


def log(msg):
    timestamp = datetime.datetime.now()
    print("{} - {}".format(timestamp, msg))
    return


def invoke(command, quiet=False):
    if not quiet:
        log("invoking command: {}".format(command))
    invoke_lock.acquire()
    try:
        result = subprocess.check_output(['/bin/bash', '-i', '-c',
                                          command]).decode("utf-8").rstrip()
        return result
    except subprocess.CalledProcessError as e:
        log("command returned non-zero code - that is expected since the node is temporarily down"
            )
        return ""
    finally:
        invoke_lock.release()


def compile_node():
    log("*** building nodes ***")
    command = "nctl-compile"
    invoke(command)


def start_network():
    log("*** starting network ***")
    command = "nctl-assets-teardown && nctl-assets-setup && RUST_LOG=debug nctl-start"
    invoke(command)


def get_chain_height(node):
    command = "nctl-view-chain-height node={}".format(node)
    result = invoke(command, True)
    m = re.match(r'.* = ([0-9]*)', result)
    if m and m.group(1):
        return int(m.group(1))
    return 0


def wait_for_height(target_height):
    log("*** waiting for height {} for {} secs ***".format(
        target_height, PROGRESS_WAIT_TIMEOUT_SECS))
    retries = PROGRESS_WAIT_TIMEOUT_SECS / 2
    while True:
        heights = []
        for node in range(1, 6):
            height = get_chain_height(node)
            heights.append(height)
        keep_waiting = len(
            list(filter(lambda height: height < target_height, heights))) != 0
        if not keep_waiting:
            return
        retries -= 1
        if retries == 0:
            log("*** ERROR: network didn't reach height {} in {} secs (current node heights: {}) ***"
                .format(target_height, PROGRESS_WAIT_TIMEOUT_SECS, heights))
            os._exit(1)
        if retries % 10 == 0:
            log("*** still waiting for height {} for {} secs (current node heights: {}) ***"
                .format(target_height, int(retries * 2), heights))
        sleep(2)


def deploy_sender_thread(count, interval):
    global current_node_count
    command = "nctl-transfer-wasm node={} transfers=1"
    while True:
        for i in range(count):
            nctl_call = command.format(random.randint(1, current_node_count))
            invoke(nctl_call, True)
        log("sent " + str(count) + " deploys and sleeping " + str(interval) +
            " seconds")
        sleep(interval)
    return


def huge_deploy_sender_thread(count, interval):
    global current_node_count

    while True:
        for i in range(count):
            random_node = random.randint(1, current_node_count)
            huge_deploy_path = make_huge_deploy(random_node)
            command = "{} send-deploy --input {} --node-address http://{} > /dev/null 2>&1".format(
                path_to_client, huge_deploy_path,
                get_node_rpc_endpoint(random_node))
            invoke(command)

        log("sent " + str(count) + " huge deploys and sleeping " +
            str(interval) + " seconds")
        sleep(interval)
    return


def get_node_rpc_endpoint(node):
    command = "nctl-view-node-ports node={}".format(node)
    result = invoke(command, True)
    m = re.match(r'.*RPC @ (\d*).*', result)
    if m and m.group(1):
        return "localhost:{}/rpc/".format(int(m.group(1)))
    return


def start_sending_deploys():
    log("*** starting sending deploys ***")
    handle = threading.Thread(target=deploy_sender_thread,
                              args=(DEPLOY_SPAM_COUNT,
                                    DEPLOY_SPAM_INTERVAL_SECS))
    handle.daemon = True
    handle.start()
    return handle


def start_sending_huge_deploys():
    log("*** starting sending huge deploys ***")
    handle = threading.Thread(target=huge_deploy_sender_thread,
                              args=(HUGE_DEPLOY_SPAM_COUNT,
                                    HUGE_DEPLOY_SPAM_INTERVAL_SECS))
    handle.daemon = True
    handle.start()
    return handle


def test_timer_thread(secs):
    sleep(secs)
    log("*** " + str(secs) + " secs passed - running health checks ***")
    run_health_checks()
    log("*** test finished successfully ***")
    os._exit(0)


def run_health_checks():
    global current_node_count
    logs_with_chunk_indicator = 0
    for node in range(1, current_node_count):
        chunk_indicator_found = False
        path_to_logs = "./utils/nctl/assets/net-1/nodes/node-{}/logs".format(
            node)
        for filename in os.listdir(path_to_logs):
            if filename != "stderr.log":
                handle = open(path_to_logs + "/" + filename, "r")
                for line in handle:
                    if re.search("chunk #3", line):
                        chunk_indicator_found = True
                handle.close()
        if not chunk_indicator_found:
            log("*** didn't find 'chunk #3' in log files of node {} ***".
                format(node))
        else:
            log("*** found 'chunk #3' in log files of node {} ***".format(
                node))
            logs_with_chunk_indicator += 1

    # We might get extremely unlucky with the random number generator and
    # send all huge deploys to the same node. In such case, logs from this
    # node will not contain chunking indicator.
    # To avoid test flakiness, we allow half of the nodes to not
    # have the chunking indicator in the logs after test finishes.
    if logs_with_chunk_indicator == 0:
        log("*** at least one node should have chunking indicator in logs")
        os._exit(1)
    return


def start_test_timer(secs):
    log("*** starting test timer (" + str(secs) + " secs) ***")
    handle = threading.Thread(target=test_timer_thread, args=(secs, ))
    handle.daemon = True
    handle.start()
    return handle


def make_huge_deploy(node):
    log("*** creating huge deploy ***")

    secret_key = "./utils/nctl/assets/net-1/nodes/node-{}/keys/secret_key.pem".format(
        node)
    session_path = "./utils/nctl/sh/scenarios/smart_contracts/named_keys_bloat.wasm"
    output = "{}.json".format(session_path)
    chain_name = "casper-net-1"
    payment_amount = 10000000000000000
    ttl = "5minutes"

    if os.path.exists(output):
        os.remove(output)
    command = "{} make-deploy --output {} --chain-name {} --payment-amount {} --ttl {} --secret-key {} --session-path {} > /dev/null 2>&1".format(
        path_to_client, output, chain_name, payment_amount, ttl, secret_key,
        session_path)
    invoke(command)
    return output


def prepare_test_env():
    compile_node()
    start_network()
    wait_for_height(2)
    timer_thread = start_test_timer(TEST_DURATION_SECS)
    deploy_sender_handle = start_sending_deploys()
    huge_deploy_sender_handle = start_sending_huge_deploys()
    return


def stop_node(node):
    log("*** stopping node {} ***".format(node))
    command = "nctl-stop node={}".format(node)
    invoke(command)


def start_node(node):
    log("*** starting node {} ***".format(node))
    command = "nctl-view-chain-lfb"
    result = invoke(command)
    trusted_hash = None
    for line in result.splitlines():
        if line.endswith("'N/A'"):
            continue
        tokens = line.split()
        trusted_hash = tokens[-1]
        break
    if trusted_hash is not None:
        command = "nctl-start node={} hash={}".format(node, trusted_hash)
        log(command)
        invoke(command)
    else:
        log("ERROR: getting trusted hash")


def wait_until_nodes_settle_on_the_same_height(node_count):
    retries = PROGRESS_WAIT_TIMEOUT_SECS / 2
    while True:
        heights = []
        for node in range(1, node_count + 1):
            height = get_chain_height(node)
            heights.append(height)
        if len(heights) == node_count and all(x == heights[0]
                                              for x in heights):
            return heights[0]
        retries -= 1
        if retries == 0:
            log("*** ERROR: nodes didn't settle on the equal heights in {} secs (current node heights: {}) ***"
                .format(PROGRESS_WAIT_TIMEOUT_SECS, heights))
            os._exit(1)
        if retries % 10 == 0:
            log("*** still waiting for nodes to settle on the same height for {} secs (current node heights: {}) ***"
                .format(int(retries * 2), heights))

        sleep(2)


def assert_network_is_progressing(node_count):
    log("*** asserting network is progressing ***")
    current_height = wait_until_nodes_settle_on_the_same_height(node_count)
    target_height = current_height + 5
    wait_for_height(target_height)
    log("network correctly progressed from {} to {}".format(
        current_height, target_height))
    return


def join_node(current_node_count):
    if current_node_count >= 10:
        log("*** not joining new node, 10 already in the network ***")
        return current_node_count

    current_node_count += 1
    log("*** joining node {} ***".format(current_node_count))
    start_node(current_node_count)
    return current_node_count


path_to_client = invoke("get_path_to_client")

prepare_test_env()

while True:
    disturbance_1(current_node_count)
    assert_network_is_progressing(current_node_count)
    sleep(DISTURBANCE_INTERVAL_SECS)

    disturbance_2(current_node_count)
    assert_network_is_progressing(current_node_count)
    sleep(DISTURBANCE_INTERVAL_SECS)

    current_node_count = join_node(current_node_count)
    assert_network_is_progressing(current_node_count)
