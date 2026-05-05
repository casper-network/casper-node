// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

contract SelfDestruct {
    uint256 public value;

    constructor() payable {
        value = 7;
    }

    function destroy(address payable beneficiary) external {
        selfdestruct(beneficiary);
    }
}
