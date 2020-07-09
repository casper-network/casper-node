use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn check_edge_cases() {
        let t = trybuild::TestCases::new();
        t.compile_fail("tests/templates/multiple_constructors.rs");
        t.compile_fail("tests/templates/no_contract.rs");
        t.compile_fail("tests/templates/no_constructor.rs");
    }

    #[test]
    fn check_simple_output() {
        let location = Path::new("../contracts/integration/sample_contract/src");
        let expansion = Command::new("cargo")
            .current_dir(location)
            .arg("expand")
            .stdout(Stdio::piped())
            .output()
            .expect("Failed to execute command");
        let template = fs::read_to_string("tests/templates/simple_contract.template")
            .expect("Failed to read template file");
        assert_eq!(String::from_utf8_lossy(&expansion.stdout), template);
    }
}
