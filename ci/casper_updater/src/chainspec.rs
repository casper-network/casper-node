use std::{
    convert::TryFrom,
    io::{self, Write},
};

use regex::Regex;

use casper_types::SemVer;

use crate::{package, regex_data};

const CAPTURE_INDEX: usize = 2;

/// Represents the production chainspec which may need its protocol version and activation point
/// updated.
pub struct Chainspec {
    /// The current production protocol version.
    current_protocol_version: SemVer,
    /// The current production activation point.
    current_activation_point: String,
}

impl Chainspec {
    pub fn new() -> Self {
        let chainspec_path = crate::root_dir().join("resources/production/chainspec.toml");

        let chainspec = &regex_data::chainspec_protocol_version::DEPENDENT_FILES[0];

        let find_value = |regex: &Regex| {
            regex
                .captures(chainspec.contents())
                .unwrap_or_else(|| {
                    panic!(
                        "should find protocol version and activation point in {}",
                        chainspec_path.display()
                    )
                })
                .get(CAPTURE_INDEX)
                .unwrap_or_else(|| {
                    panic!(
                        "protocol version and activation point should be regex capture at index {} in {}",
                        CAPTURE_INDEX,
                        chainspec_path.display()
                    )
                })
                .as_str()
                .to_string()
        };

        let protocol_version = find_value(&*regex_data::chainspec_protocol_version::REGEX);
        let current_activation_point = find_value(&*regex_data::chainspec_activation_point::REGEX);
        let current_protocol_version = SemVer::try_from(protocol_version.as_str())
            .expect("should parse current protocol version");

        Chainspec {
            current_protocol_version,
            current_activation_point,
        }
    }

    pub fn update(&self) {
        if crate::is_dry_run() {
            println!(
                "Current protocol version is {}",
                self.current_protocol_version
            );
            if let Some(bump_version) = crate::bump_version() {
                let updated_protocol_version = bump_version.update(self.current_protocol_version);
                println!("Will be updated to {}", updated_protocol_version);
            }
            println!("Files affected by the protocol version:");
            for dependent_file in &*regex_data::chainspec_protocol_version::DEPENDENT_FILES {
                println!("\t* {}", dependent_file.relative_path().display());
            }
            println!();
            println!(
                "Current activation point is {}",
                self.current_activation_point
            );
            if let Some(activation_point) = crate::new_activation_point() {
                println!("Will be updated to {}", activation_point);
            }
            println!("Files affected by the activation point:");
            for dependent_file in &*regex_data::chainspec_activation_point::DEPENDENT_FILES {
                println!("\t* {}", dependent_file.relative_path().display());
            }
            println!();
            return;
        }

        self.update_protocol_version();
        self.update_activation_point();
    }

    fn update_protocol_version(&self) {
        let updated_protocol_version = match crate::bump_version() {
            None => match package::get_updated_version_from_user(
                "chainspec protocol",
                self.current_protocol_version,
            ) {
                Some(version) => version,
                None => return,
            },
            Some(bump_version) => bump_version.update(self.current_protocol_version),
        };

        for dependent_file in &*regex_data::chainspec_protocol_version::DEPENDENT_FILES {
            dependent_file.update(&updated_protocol_version.to_string());
        }

        println!(
            "Updated protocol version from {} to {}.",
            self.current_protocol_version, updated_protocol_version
        );
    }

    fn update_activation_point(&self) {
        let updated_activation_point = match crate::new_activation_point() {
            None => match self.get_updated_activation_point_from_user() {
                Some(activation_point) => activation_point,
                None => return,
            },
            Some(activation_point) => activation_point,
        };

        for dependent_file in &*regex_data::chainspec_activation_point::DEPENDENT_FILES {
            dependent_file.update(&updated_activation_point.to_string());
        }

        println!(
            "Updated activation point from {} to {}.",
            self.current_activation_point, updated_activation_point
        );
    }

    fn get_updated_activation_point_from_user(&self) -> Option<u64> {
        loop {
            print!(
                "Current chainspec activation point is {}.  Enter new activation point (leave blank for unchanged): ",
                self.current_activation_point
            );
            io::stdout().flush().expect("should flush stdout");
            let mut input = String::new();
            match io::stdin().read_line(&mut input) {
                Ok(_) => {
                    input = input.trim_end().to_string();
                    if input.is_empty() {
                        return None;
                    }

                    let new_activation_point = match input.parse() {
                        Ok(activation_point) => activation_point,
                        Err(error) => {
                            println!("\n{} is not a valid unsigned integer: {}.", input, error);
                            continue;
                        }
                    };

                    return if input == self.current_activation_point {
                        None
                    } else {
                        Some(new_activation_point)
                    };
                }
                Err(error) => println!("\nFailed to read from stdin: {}.", error),
            }
        }
    }
}
