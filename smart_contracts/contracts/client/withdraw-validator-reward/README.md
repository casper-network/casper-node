# withdraw-validator-reward

Example usage:
```
casper-client put-deploy \
    --chain-name casper-example \
    --payment-amount 1000000000000 \
    --secret-key resources/local/secret_keys/node-1.pem \
    --session-path target/wasm32-unknown-unknown/release/withdraw_validator_reward.wasm \
    --session-arg validator_public_key:public_key='01f60bce2bb1059c41910eac1e7ee6c3ef4c8fcc63a901eb9603c1524cadfb0c18' \
    --session-arg target_purse:opt_uref=null
```
