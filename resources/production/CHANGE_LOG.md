# Change Log

## casper-node_1.5.0_amd64.deb

 - pre-Delta test release
 - debian packaging enhancement for automatic configuration.

## casper-node_1.4.0_amd64.deb

 - pre-Delta test release
 - bonding and unbonding added
 - Actions Required:
    - Update config.toml (see config.example.toml):
        - network
            - gossip_interval: Change from 30000 to 120000 (30 to 120 secs)
        - gossip
            - finished_entry_duration_secs: Change from 3600 to 60 (1 hour to 60 secs)
            - gossip_request_timeout_secs: Change from 10 to 30 (10 to 30 secs)
   
## casper-node_0.1.0-0_amd64.deb

 - Initial Release
 - Actions Required:
    - Create '/etc/casper/config.toml' from '/etc/casper/config-example.toml'
    - Inside '/etc/casper/validator_keys', follow README.md and send back public_key_hex contents.
      You will only need to generate keys once and can reuse them.