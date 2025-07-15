mod calling_packages_by_version_query;
mod chainspec_registry;
mod check_transfer_success;
mod contract_api;
mod contract_context;
mod contract_messages;
mod counter_factory;
mod deploy;
mod explorer;
mod get_balance;
mod groups;
mod host_function_costs;
mod manage_groups;
mod private_chain;
mod regression;
mod ret;
mod stack_overflow;
mod step;
mod storage_costs;
mod system_contracts;
mod system_costs;
mod tutorial;
mod upgrade;
mod wasmless_transfer;

// NOTE: the original execution engine also handled charging for gas costs
// and these integration tests commonly would, in addition to other behavior being tested,
// also check that expected payment handling was being done.
// As of 2.0 compliant execution engines no longer handle payment...
// all payment handling is done in the node prior to engaging native logic or an execution target
// and all testing of payment handling occurs within the node tests.
// Thus these ee integration tests cannot (and should not) test changes to balances related
// to costs as they once did. Instead they should (and only can) test that gas limits are
// correctly applied and enforced and that non-cost transfers work properly.
// Because many tests included balance checks with expectations around payment handling in
// addition to whatever else they were testing, they required adjustment.
// In some cases the names of the tests included terms such as 'should_charge_' or 'should_cost_'
// which is no longer true and require the name of the test be adjusted to reflect the new reality.
