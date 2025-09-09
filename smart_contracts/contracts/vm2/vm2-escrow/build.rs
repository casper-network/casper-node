use casper_contract_sdk::build;

fn main() {
    build::BuildConfig::from_env().emit();
}
