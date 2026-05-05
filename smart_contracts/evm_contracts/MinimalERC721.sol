// SPDX-License-Identifier: MIT
pragma solidity ^0.8.24;

contract MinimalERC721 {
    string public name = "Casper EVM Test NFT";
    string public symbol = "CEN";

    mapping(uint256 => address) private owners;
    mapping(address => uint256) private balances;
    mapping(uint256 => address) private tokenApprovals;

    event Transfer(address indexed from, address indexed to, uint256 indexed tokenId);
    event Approval(address indexed owner, address indexed approved, uint256 indexed tokenId);

    function mint(address to, uint256 tokenId) external {
        require(to != address(0), "zero");
        require(owners[tokenId] == address(0), "minted");
        owners[tokenId] = to;
        balances[to] += 1;
        emit Transfer(address(0), to, tokenId);
    }

    function balanceOf(address owner) external view returns (uint256) {
        require(owner != address(0), "zero");
        return balances[owner];
    }

    function ownerOf(uint256 tokenId) public view returns (address) {
        address owner = owners[tokenId];
        require(owner != address(0), "missing");
        return owner;
    }

    function approve(address to, uint256 tokenId) external {
        address owner = ownerOf(tokenId);
        require(msg.sender == owner, "owner");
        tokenApprovals[tokenId] = to;
        emit Approval(owner, to, tokenId);
    }

    function getApproved(uint256 tokenId) external view returns (address) {
        require(owners[tokenId] != address(0), "missing");
        return tokenApprovals[tokenId];
    }

    function transferFrom(address from, address to, uint256 tokenId) public {
        address owner = ownerOf(tokenId);
        require(owner == from, "from");
        require(to != address(0), "zero");
        require(msg.sender == owner || msg.sender == tokenApprovals[tokenId], "auth");

        tokenApprovals[tokenId] = address(0);
        balances[from] -= 1;
        balances[to] += 1;
        owners[tokenId] = to;
        emit Transfer(from, to, tokenId);
    }
}
