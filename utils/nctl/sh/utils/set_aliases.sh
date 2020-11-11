# Assets.
alias nctl-assets-dump='source $NCTL/sh/assets/dump.sh'
alias nctl-assets-ls='source $NCTL/sh/assets/list.sh'
alias nctl-assets-setup='source $NCTL/sh/assets/setup.sh'
alias nctl-assets-teardown='source $NCTL/sh/assets/teardown.sh'

# Compilation.
alias nctl-compile='source $NCTL/sh/compile/all.sh'
alias nctl-compile-client='source $NCTL/sh/compile/client.sh'
alias nctl-compile-node='source $NCTL/sh/compile/node.sh'

# Workload Generators.
alias nctl-wg-100='source $NCTL/sh/generators/wg_100.sh'
alias nctl-do-transfer='source $NCTL/sh/generators/wg_100.sh'

alias nctl-wg-110='source $NCTL/sh/generators/wg_110.sh'
alias nctl-do-transfer-wasm='source $NCTL/sh/generators/wg_110.sh'

alias nctl-wg-200='source $NCTL/sh/generators/wg_200.sh'
alias nctl-do-auction-submit='source $NCTL/sh/generators/wg_200.sh'

alias nctl-wg-201='source $NCTL/sh/generators/wg_201.sh'
alias nctl-do-auction-withdraw='source $NCTL/sh/generators/wg_201.sh'

alias nctl-wg-210='source $NCTL/sh/generators/wg_210.sh'
alias nctl-do-auction-delegate='source $NCTL/sh/generators/wg_210.sh'

alias nctl-wg-211='source $NCTL/sh/generators/wg_211.sh'
alias nctl-do-auction-undelegate='source $NCTL/sh/generators/wg_211.sh'

# Logs.
alias nctl-log-reset='source $NCTL/sh/node/log_reset.sh'

# Node.
alias nctl-down='source $NCTL/sh/node/stop.sh'
alias nctl-interactive='source $NCTL/sh/node/interactive.sh'
alias nctl-restart='source $NCTL/sh/node/restart.sh'
alias nctl-start='source $NCTL/sh/node/start.sh'
alias nctl-status='source $NCTL/sh/node/status.sh'
alias nctl-stop='source $NCTL/sh/node/stop.sh'
alias nctl-toggle='source $NCTL/sh/node/toggle.sh'
alias nctl-up='source $NCTL/sh/node/start.sh'

# Views: chain.
alias nctl-view-chain-account='source $NCTL/sh/views/view_chain_account.sh'
alias nctl-view-chain-account-balance='source $NCTL/sh/views/view_chain_account_balance.sh'
alias nctl-view-chain-auction-info='source $NCTL/sh/views/view_chain_auction_info.sh'
alias nctl-view-chain-block='source $NCTL/sh/views/view_chain_block.sh'
alias nctl-view-chain-deploy='source $NCTL/sh/views/view_chain_deploy.sh'
alias nctl-view-chain-state-root-hash='source $NCTL/sh/views/view_chain_state_root_hash.sh'
alias nctl-view-chain-spec='source $NCTL/sh/views/view_chain_spec.sh'
alias nctl-view-chain-spec-accounts='source $NCTL/sh/views/view_chain_spec_accounts.sh'

# Views: node.
alias nctl-view-node-config='source $NCTL/sh/views/view_node_config.sh'
alias nctl-view-node-log='source $NCTL/sh/views/view_node_log.sh'
alias nctl-view-node-peers='source $NCTL/sh/views/view_node_peers.sh'
alias nctl-view-node-status='source $NCTL/sh/views/view_node_status.sh'
alias nctl-view-node-storage='source $NCTL/sh/views/view_node_storage.sh'

# Views: node metrcs.
alias nctl-view-node-metrics='source $NCTL/sh/views/view_node_metrics.sh'
alias nctl-view-node-metric-pending-deploy='source $NCTL/sh/views/view_node_metrics.sh metric=pending_deploy'
alias nctl-view-node-metric-finalised-block-count='source $NCTL/sh/views/view_node_metrics.sh metric=amount_of_blocks'

# Views: network.
alias nctl-view-faucet-account='source $NCTL/sh/views/view_faucet_account.sh'
alias nctl-view-faucet-account-balance='source $NCTL/sh/views/view_faucet_account_balance.sh'
alias nctl-view-faucet-account-hash='source $NCTL/sh/views/view_faucet_account_hash.sh'
alias nctl-view-faucet-account-key='source $NCTL/sh/views/view_faucet_account_key.sh'
alias nctl-view-faucet-secret-key-path='source $NCTL/sh/views/view_faucet_secret_key_path.sh'

# Views: user.
alias nctl-view-user-account='source $NCTL/sh/views/view_user_account.sh'
alias nctl-view-user-account-balance='source $NCTL/sh/views/view_user_account_balance.sh'
alias nctl-view-user-account-hash='source $NCTL/sh/views/view_user_account_hash.sh'
alias nctl-view-user-account-key='source $NCTL/sh/views/view_user_account_key.sh'
alias nctl-view-user-secret-key-path='source $NCTL/sh/views/view_user_secret_key_path.sh'

# Views: validator.
alias nctl-view-validator-account='source $NCTL/sh/views/view_validator_account.sh'
alias nctl-view-validator-account-balance='source $NCTL/sh/views/view_validator_account_balance.sh'
alias nctl-view-validator-account-hash='source $NCTL/sh/views/view_validator_account_hash.sh'
alias nctl-view-validator-account-key='source $NCTL/sh/views/view_validator_account_key.sh'
alias nctl-view-validator-secret-key-path='source $NCTL/sh/views/view_validator_secret_key_path.sh'
