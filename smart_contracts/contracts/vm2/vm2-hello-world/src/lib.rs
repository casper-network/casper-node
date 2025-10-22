#![no_std]
#![no_main]

extern crate alloc;

use casper_contract_sdk::prelude::*;

const GREETING_ENG: &str = "Hello world";
const GREETING_ES: &str = "Hola Mundo";
const GREETING_FR: &str = "Bonjour le monde";

#[casper(contract_state)]
pub struct HelloWorldContract {
    greeting: String,
}

impl Default for HelloWorldContract {
    fn default() -> Self {
        Self {
            greeting: GREETING_ENG.to_string(),
        }
    }
}

#[casper]
impl HelloWorldContract {
    #[casper(constructor)]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spanish(&mut self) {
        self.greeting = GREETING_ES.to_string();
    }

    pub fn french(&mut self) {
        self.greeting = GREETING_FR.to_string();
    }

    pub fn get(&self) -> String {
        self.greeting.clone()
    }
}
