use humantime::format_rfc3339;
use std::{
    env, fs,
    path::Path,
    time::{Duration, SystemTime},
};
use toml_edit::{value, Document};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let input_chainspec = Path::new(&manifest_dir)
        .join("resources")
        .join("chainspec.toml.in");
    let output_chainspec = Path::new(&manifest_dir)
        .join("resources")
        .join("chainspec.toml");

    println!("cargo:rerun-if-changed={}", input_chainspec.display());

    let toml = fs::read_to_string(input_chainspec).expect("could not read chainspec.toml.in");
    let mut doc = toml
        .parse::<Document>()
        .expect("invalid document in chainspec.toml.in");
    let activation_point = SystemTime::now() + Duration::from_secs(40);
    doc["protocol"]["activation_point"] = value(format_rfc3339(activation_point).to_string());

    fs::write(output_chainspec, doc.to_string()).expect("could not write chainspec.toml");
}
