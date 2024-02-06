use casper_sdk::cli;
use vm2_cep18::contract::CEP18;

fn main() {
    cli::command_line::<CEP18>();
}
