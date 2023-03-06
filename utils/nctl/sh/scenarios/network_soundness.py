#!/usr/bin/env python3

import os
import random
import re
import subprocess
import sys
import threading
from time import sleep

TEST_DURATION_SECS = 2 * 60
DEPLOY_SPAM_INTERVAL_SECS = 3 * 60
DEPLOY_SPAM_COUNT = 200
HARASSMENT_INTERVAL_SECS = 5

invoke_lock = threading.Lock()


def invoke(command):
    invoke_lock.acquire()
    result = subprocess.check_output(['/bin/bash', '-i', '-c',
                                      command]).decode("utf-8").rstrip()
    invoke_lock.release()
    return result


def compile_node():
    print("*** building nodes ***")
    command = "nctl-compile"
    invoke(command)


def start_network():
    print("*** starting network ***")
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
    print("*** waiting for height {} ***".format(target_height))
    while True:
        heights = []
        for node in range(1, 6):
            height = get_chain_height(node)
            heights.append(height)
        keep_waiting = len(
            list(filter(lambda height: height < target_height, heights))) != 0
        if not keep_waiting:
            return
        sleep(2)


def deploy_sender_thread(count, interval):
    command = "nctl-transfer-wasm node={} transfers=1"
    while True:
        for i in range(count):
            nctl_call = command.format(random.randint(1, 5))
            invoke(nctl_call)
        print("sent " + str(count) + " deploys and sleeping " + str(interval) +
              " seconds")
        sleep(interval)
    return


def start_sending_deploys():
    print("*** starting sending deploys ***")
    handle = threading.Thread(target=deploy_sender_thread,
                              args=(DEPLOY_SPAM_COUNT,
                                    DEPLOY_SPAM_INTERVAL_SECS))
    handle.daemon = True
    handle.start()
    return handle


def test_timer_thread(secs):
    sleep(secs)
    print("*** " + str(secs) + " secs passed - finishing test ***")
    os._exit(0)
    return


def start_test_timer(secs):
    print("*** starting test timer (" + str(secs) + " secs) ***")
    handle = threading.Thread(target=test_timer_thread, args=(secs, ))
    handle.daemon = True
    handle.start()
    return handle


def prepare_test_env():
    #    compile_node()
    #    start_network()
    wait_for_height(2)
    timer_thread = start_test_timer(TEST_DURATION_SECS)
    deploy_sender_handle = start_sending_deploys()


# Kill a random node, wait one minute, restart node
def harassment_1():
    # TODO
    print("*** starting harassment #1 ***")
    return


# Kill two random nodes, wait one minute, restart nodes
def harassment_2():
    print("*** starting harassment #2 ***")
    # TODO
    return


def wait_until_nodes_settle_on_the_same_height(node_count):
    while True:
        heights = []
        for node in range(1, node_count + 1):
            height = get_chain_height(node)
            heights.append(height)
        if len(heights) == node_count and all(x == heights[0]
                                              for x in heights):
            return heights[0]
        sleep(2)


# Kill two random nodes, wait one minute, restart nodes
def assert_network_is_progressing(node_count):
    print("*** asserting network is progressing ***")
    current_height = wait_until_nodes_settle_on_the_same_height(node_count)
    target_height = current_height + 5
    wait_for_height(target_height)
    print("network correctly progressed from {} to {}".format(
        current_height, target_height))
    return


prepare_test_env()
current_node_count = 5
while True:
    harassment_1()
    assert_network_is_progressing(current_node_count)
    sleep(HARASSMENT_INTERVAL_SECS)
