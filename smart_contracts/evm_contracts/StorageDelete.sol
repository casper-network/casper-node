// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

contract StorageDelete {
    uint256 public value;

    function set(uint256 newValue) external {
        value = newValue;
    }

    function clear() external {
        delete value;
    }
}
