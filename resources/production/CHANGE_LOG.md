# Change Log

## casper-node_1.4.0_amd64.deb

 - pre-Delta test release
 - Changed to major version of 1 to work better with Rust versioning.
    - 0.1.3 -> 1.4.0
 - bonding and unbonding added with auction
 - Actions Required:
    - Update config.toml (we recommend starting with new config.example.toml, as there are many changes):
   
## casper-node_0.1.0-0_amd64.deb

 - Initial Release
 - Actions Required:
    - Create '/etc/casper/config.toml' from '/etc/casper/config-example.toml'
    - Inside '/etc/casper/validator_keys', follow README.md and send back public_key_hex contents.
      You will only need to generate keys once and can reuse them.