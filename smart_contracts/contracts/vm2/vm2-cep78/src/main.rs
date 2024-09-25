use casper_sdk::cli;
use vm2_cep78::contract::NFTContract;

fn main() {
    cli::command_line::<NFTContract>();
}
