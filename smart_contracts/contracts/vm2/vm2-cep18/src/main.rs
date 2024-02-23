use casper_sdk::cli;
use vm2_cep18::contract::TokenContract;

fn main() {
    cli::command_line::<TokenContract>();
}
