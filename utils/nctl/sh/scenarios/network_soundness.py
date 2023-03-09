#!/usr/bin/env python3

import datetime
import os
import random
import re
import subprocess
import sys
import threading
from time import sleep

TEST_DURATION_SECS = 30 * 60
DEPLOY_SPAM_INTERVAL_SECS = 3 * 60
DEPLOY_SPAM_COUNT = 200
HARASSMENT_INTERVAL_SECS = 5
PROGRESS_WAIT_TIMEOUT_SECS = 60

invoke_lock = threading.Lock()


# Kill a random node, wait one minute, restart node
def harassment_1(node_count):
    log("*** starting harassment type 1 ***")
    random_node = random.randint(1, node_count)
    stop_node(random_node)
    sleep(60)
    start_node(random_node)
    return


# Kill two random nodes, wait one minute, restart nodes
def harassment_2(node_count):
    log("*** starting harassment type 2 ***")
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


def invoke(command):
    invoke_lock.acquire()
    result = subprocess.check_output(['/bin/bash', '-i', '-c',
                                      command]).decode("utf-8").rstrip()
    invoke_lock.release()
    return result


def compile_node():
    log("*** building nodes ***")
    command = "nctl-compile"
    invoke(command)


def start_network():
    log("*** starting network ***")
    command = "nctl-assets-teardown && NCTL_NODE_LOG_FORMAT=text nctl-assets-setup && RUST_LOG=debug nctl-start"
    invoke(command)


def get_chain_height(node):
    command = "nctl-view-chain-height node={}".format(node)
    result = invoke(command)
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
    command = "nctl-transfer-wasm node={} transfers=1"
    while True:
        for i in range(count):
            nctl_call = command.format(random.randint(1, 5))
            invoke(nctl_call)
        log("sent " + str(count) + " deploys and sleeping " + str(interval) +
            " seconds")
        sleep(interval)
    return


def start_sending_deploys():
    log("*** starting sending deploys ***")
    handle = threading.Thread(target=deploy_sender_thread,
                              args=(DEPLOY_SPAM_COUNT,
                                    DEPLOY_SPAM_INTERVAL_SECS))
    handle.daemon = True
    handle.start()
    return handle


def test_timer_thread(secs):
    sleep(secs)
    log("*** " + str(secs) + " secs passed - finishing test ***")
    os._exit(0)
    return


def start_test_timer(secs):
    log("*** starting test timer (" + str(secs) + " secs) ***")
    handle = threading.Thread(target=test_timer_thread, args=(secs, ))
    handle.daemon = True
    handle.start()
    return handle


def prepare_test_env():
    compile_node()
    start_network()
    wait_for_height(2)
    timer_thread = start_test_timer(TEST_DURATION_SECS)
    deploy_sender_handle = start_sending_deploys()


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


prepare_test_env()
current_node_count = 5
while True:
    current_node_count = join_node(current_node_count)
    assert_network_is_progressing(current_node_count)
    harassment_1(current_node_count)
    # assert_network_is_progressing(current_node_count)
    # sleep(HARASSMENT_INTERVAL_SECS)
    # harassment_2(current_node_count)
    # assert_network_is_progressing(current_node_count)
    # sleep(HARASSMENT_INTERVAL_SECS)
    # sleep(HARASSMENT_INTERVAL_SECS)
