use std::env;

fn main() {
    match env::var("TARGET") {
        Ok(target) => {
            println!("cargo:rustc-env=TARGET={}", target);
        }
        Err(_) => {
            println!("cargo:warning=Failed to obtain target triple");
        }
    }
}
