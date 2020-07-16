use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

trait Package {
    const ROOT: &'static str;
    const CARGO_TOML: &'static str;
    const MAIN_RS: &'static str;
    const WASM_FILENAME: &'static str;
}

struct MintInstall;

impl Package for MintInstall {
    const ROOT: &'static str = "../contracts/system/mint-install";
    const CARGO_TOML: &'static str = "../contracts/system/mint-install/Cargo.toml";
    const MAIN_RS: &'static str = "../contracts/system/mint-install/src/main.rs";
    const WASM_FILENAME: &'static str = "mint_install.wasm";
}

struct PosInstall;

impl Package for PosInstall {
    const ROOT: &'static str = "../contracts/system/pos-install";
    const CARGO_TOML: &'static str = "../contracts/system/pos-install/Cargo.toml";
    const MAIN_RS: &'static str = "../contracts/system/pos-install/src/main.rs";
    const WASM_FILENAME: &'static str = "pos_install.wasm";
}

struct StandardPayment;

impl Package for StandardPayment {
    const ROOT: &'static str = "../contracts/system/standard-payment";
    const CARGO_TOML: &'static str = "../contracts/system/standard-payment/Cargo.toml";
    const MAIN_RS: &'static str = "../contracts/system/standard-payment/src/bin/main.rs";
    const WASM_FILENAME: &'static str = "standard_payment.wasm";
}

struct StandardPaymentInstall;

impl Package for StandardPaymentInstall {
    const ROOT: &'static str = "../contracts/system/standard-payment-install";
    const CARGO_TOML: &'static str = "../contracts/system/standard-payment-install/Cargo.toml";
    const MAIN_RS: &'static str = "../contracts/system/standard-payment-install/src/main.rs";
    const WASM_FILENAME: &'static str = "standard_payment_install.wasm";
}

const TARGET_DIR_FOR_WASM: &str = "target/built-contracts";
const ORIGINAL_WASM_DIR: &str = "wasm32-unknown-unknown/release";
const NEW_WASM_DIR: &str = "wasm";

fn build_package<T: Package>() {
    // Watch contract source files for changes.
    println!("cargo:rerun-if-changed={}", T::CARGO_TOML);
    println!("cargo:rerun-if-changed={}", T::MAIN_RS);

    // Full path to the cargo binary.
    let cargo = env::var("CARGO").expect("env var 'CARGO' should be set");
    // Full path to the 'execution-engine/cargo-casperlabs' dir.
    let root_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("env var 'CARGO_MANIFEST_DIR' should be set"),
    );

    let mut build_args = vec!["build".to_string(), "--release".to_string()];

    // We can't build the contract right into the normal target dir since cargo has a lock on
    // this while building 'cargo-casperlabs'.  Instead, we'll build to
    // '.../cargo-casperlabs/target/built-contracts' and then copy the resulting Wasm file from
    // there to '.../cargo-casperlabs/wasm'.

    let target_dir = root_dir.join(TARGET_DIR_FOR_WASM);
    build_args.push(format!(
        "--target-dir={}",
        target_dir.to_str().expect("Expected valid unicode")
    ));

    // Build the contract.
    let output = Command::new(cargo)
        .current_dir(T::ROOT)
        .args(build_args)
        .output()
        .unwrap_or_else(|_| panic!("Expected to build {}", T::WASM_FILENAME));
    assert!(
        output.status.success(),
        "Failed to build {}:\n{:?}",
        T::WASM_FILENAME,
        output
    );

    // Move the compiled Wasm file to our own folder ("cargo-casperlabs/wasm").
    let new_wasm_dir = env::current_dir().unwrap().join(NEW_WASM_DIR);
    let _ = fs::create_dir(&new_wasm_dir);

    let original_wasm_file = target_dir.join(ORIGINAL_WASM_DIR).join(T::WASM_FILENAME);
    let copied_wasm_file = new_wasm_dir.join(T::WASM_FILENAME);
    fs::copy(original_wasm_file, copied_wasm_file).unwrap();
}

fn assert_wasm_file_exists<T: Package>() {
    let root_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("env var 'CARGO_MANIFEST_DIR' should be set"),
    );
    let wasm_file = root_dir.join(NEW_WASM_DIR).join(T::WASM_FILENAME);
    assert!(wasm_file.is_file(), "{} must exist", wasm_file.display());
}

fn main() {
    let mint_install_source_exists = Path::new(MintInstall::CARGO_TOML).is_file();
    let pos_install_source_exists = Path::new(PosInstall::CARGO_TOML).is_file();
    let standard_payment_source_exists = Path::new(StandardPayment::CARGO_TOML).is_file();
    let standard_payment_install_source_exists =
        Path::new(StandardPaymentInstall::CARGO_TOML).is_file();

    match (
        mint_install_source_exists,
        pos_install_source_exists,
        standard_payment_source_exists,
        standard_payment_install_source_exists,
    ) {
        (true, true, true, true) => {
            // We're building from within CasperLabs repo - build the contracts.
            build_package::<MintInstall>();
            build_package::<PosInstall>();
            build_package::<StandardPayment>();
            build_package::<StandardPaymentInstall>();
        }
        (false, false, false, false) => {
            // We're outside the CasperLabs repo - the compiled contracts should exist locally.
            assert_wasm_file_exists::<MintInstall>();
            assert_wasm_file_exists::<PosInstall>();
            assert_wasm_file_exists::<StandardPayment>();
            assert_wasm_file_exists::<StandardPaymentInstall>();
        }
        _ => panic!("Some, but not all required contract sources exist locally."),
    }
}
