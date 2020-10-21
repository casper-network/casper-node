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

# State views.
# alias nctl-view-account='source $NCTL/sh/views/view_account.sh'
# alias nctl-view-deploy='source $NCTL/sh/views/view_deploy.sh'

alias nctl-view-node-log='source $NCTL/sh/views/view_node_log.sh'
alias nctl-view-node-metrics='source $NCTL/sh/views/view_node_metrics.sh'
alias nctl-view-node-peers='source $NCTL/sh/views/view_node_peers.sh'
alias nctl-view-node-status='source $NCTL/sh/views/view_node_status.sh'
alias nctl-view-node-storage='source $NCTL/sh/views/view_node_storage.sh'
