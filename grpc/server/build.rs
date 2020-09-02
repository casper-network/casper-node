use std::{
    env, fs,
    path::{Path, PathBuf},
};

const PROTOBUF_DIR: &str = "generated_protobuf";
const WORKAROUND_COMMENT: &str = "// workaround for https://github.com/rust-lang/rfcs/issues/752";

// The generated file needs to be sourced via `include!` which doesn't work where the file has top-
// level inner attributes (see https://github.com/rust-lang/rfcs/issues/752).
//
// To work around this issue, we add 'pub mod proto { }' around the contents.
fn wrap_file_contents(target_dir: &Path, file_name_without_suffix: &str) {
    let generated_file = target_dir
        .join(file_name_without_suffix)
        .with_extension("rs");

    let contents = fs::read_to_string(&generated_file)
        .unwrap_or_else(|_| panic!("should read {}", generated_file.display()));
    fs::write(
        &generated_file,
        &format!(
            "pub mod {} {{ {}\n\n{}\n}}",
            file_name_without_suffix, WORKAROUND_COMMENT, contents
        ),
    )
    .unwrap_or_else(|_| panic!("should write {}", generated_file.display()));
}

fn main() {
    println!("cargo:rerun-if-changed=protobuf/casper/state.proto");
    println!("cargo:rerun-if-changed=protobuf/casper/ipc.proto");
    println!("cargo:rerun-if-changed=protobuf/casper/transforms.proto");

    let target_dir = PathBuf::from(format!(
        "{}/../../../../{}",
        env::var("OUT_DIR").expect("should have env var OUT_DIR set"),
        PROTOBUF_DIR
    ));
    fs::create_dir_all(&target_dir)
        .unwrap_or_else(|_| panic!("should create dir {}", target_dir.display()));

    protoc_rust_grpc::run(protoc_rust_grpc::Args {
        out_dir: target_dir.to_str().unwrap(),
        input: &[
            "protobuf/casper/state.proto",
            "protobuf/casper/ipc.proto",
            "protobuf/casper/transforms.proto",
        ],
        includes: &["protobuf/"],
        rust_protobuf: true,
    })
    .expect("protoc-rust-grpc");

    wrap_file_contents(&target_dir, "state");
    wrap_file_contents(&target_dir, "ipc");
    wrap_file_contents(&target_dir, "transforms");
    wrap_file_contents(&target_dir, "ipc_grpc");
}
