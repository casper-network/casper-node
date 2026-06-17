// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

contract Counter {
    uint256 private counter;

    event CounterIncremented(address indexed caller, uint256 newValue);

    function increment() external payable returns (uint256) {
        counter += 1;
        emit CounterIncremented(msg.sender, counter);
        return counter;
    }

    function decrement() external payable returns (uint256) {
        counter -= 1;
        return counter;
    }

    function get() external view returns (uint256) {
        return counter;
    }
}
