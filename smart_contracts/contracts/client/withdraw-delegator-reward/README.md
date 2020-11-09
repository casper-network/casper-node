# withdraw-delegator-reward

Example usage:
```
casper-client put-deploy \
    --chain-name casper-example \
    --payment-amount 1000000000000 \
    --secret-key resources/local/secret_keys/node-4.pem \
    --session-path target/wasm32-unknown-unknown/release/withdraw_delegator_reward.wasm \
    --session-arg delegator_public_key:public_key='01b3feeec1d91c7c2f070052315258eeaaf7c24029a80a1aea285814e9f9a20d36' \
    --session-arg validator_public_key:public_key='01f60bce2bb1059c41910eac1e7ee6c3ef4c8fcc63a901eb9603c1524cadfb0c18' \
    --session-arg target_purse:opt_uref=null
```