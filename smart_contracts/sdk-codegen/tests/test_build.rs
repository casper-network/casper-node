use std::{fs, io::Write};

use casper_sdk_codegen::Codegen;

const FIXTURE_1: &str = include_str!("fixtures/cep18_schema.json");

const PROLOG: &str = "#![allow(dead_code, unused_variables, non_camel_case_types)]";
const EPILOG: &str = "fn main() {}";

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
    tmp.write_all(&code.as_bytes())?;
    tmp.flush()?;
    let t = trybuild::TestCases::new();
    t.pass(tmp.path());
    Ok(())
}
