use std::{fs, io::Write, path::PathBuf, str::FromStr};

use casper_sdk_codegen::Codegen;

const FIXTURE_1: &str = include_str!("fixtures/cep18_schema.json");

const PROLOG: &str = "#![allow(dead_code, unused_variables, non_camel_case_types)]";
const EPILOG: &str = "fn main() {}";

#[ignore = "Not yet supported"]
#[test]
fn it_works() -> Result<(), std::io::Error> {
    let mut schema = Codegen::from_str(FIXTURE_1)?;
    let mut code = schema.gen();
    code.insert_str(0, PROLOG);

    code += EPILOG;

    let mut tmp = tempfile::Builder::new()
        .prefix("cep18_schema")
        .suffix(".rs")
        .tempfile()?;
    tmp.write_all(code.as_bytes())?;

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("cep18_schema.rs");
    fs::write(path, code.as_bytes())?;
    tmp.flush()?;
    let t = trybuild::TestCases::new();
    t.pass(tmp.path());
    Ok(())
}
