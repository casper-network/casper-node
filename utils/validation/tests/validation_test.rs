use std::{
    env,
    path::{Path, PathBuf},
};

use casper_validation::{abi::ABIFixture, error::Error, test_case::TestCase, Fixture};

fn get_fixtures_path() -> PathBuf {
    let mut path = Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf();
    path.push("tests");
    path.push("fixtures");
    path
}

fn prog() -> Option<String> {
    let first_arg = env::args().next()?;
    let path = Path::new(&first_arg);
    let filename = path.file_name()?.to_str()?;
    let prog_name = match filename.split('-').next() {
        Some(name) => name,
        None => filename,
    };
    Some(prog_name.to_string())
}

fn make_abi_tests(test_name: &str, test_fixture: ABIFixture) -> Vec<(String, Box<dyn TestCase>)> {
    let prog_name = prog().expect("should get exe");

    let mut tests = Vec::with_capacity(test_fixture.len());

    for (test_case, data) in test_fixture.into_inner() {
        // validation_test::fixture_file_name::test_case
        let desc = format!("{}::{}::{}", prog_name, test_name, test_case);

        tests.push((desc, Box::new(data) as Box<dyn TestCase>));
    }

    tests
}

fn make_test_cases() -> Result<Vec<(String, Box<dyn TestCase>)>, Error> {
    let fixtures = get_fixtures_path();
    let test_fixtures = casper_validation::load_fixtures(&fixtures)?;

    let mut tests = Vec::new();

    for test_fixture in test_fixtures {
        match test_fixture {
            Fixture::ABI {
                name,
                fixture: abi_test_case,
            } => tests.append(&mut make_abi_tests(&name, abi_test_case)),
        }
    }

    Ok(tests)
}

fn main() -> anyhow::Result<()> {
    let _args: Vec<_> = env::args().collect();
    for (name, test_case) in make_test_cases()? {
        print!("{}... ", name);
        match test_case.run_test() {
            Ok(()) => println!("OK"),
            Err(error) => println!("ERROR: {}", error),
        }
    }
    Ok(())
}
