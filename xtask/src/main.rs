use std::{env, io::Write, path::PathBuf};

type DynError = Box<dyn std::error::Error>;

fn main() {
    if let Err(e) = try_main() {
        eprintln!("{}", e);
        std::process::exit(-1);
    }
}

fn try_main() -> Result<(), DynError> {
    let task = env::args().nth(1);
    match task.as_ref().map(|it| it.as_str()) {
        Some("capnpc-rust") => capnpc_rust()?,
        _ => print_help(),
    }
    Ok(())
}

fn print_help() {
    eprintln!(
        "Tasks:\n\
         capnpc-rust            compiles capnproto files to rust in capnp_types crate"
    )
}

fn capnpc_rust() -> Result<(), DynError> {
    let capnp_schemas_dir_name = "capnp_schemas";
    let capnp_types_dir_name = "capnp";
    let output_dir_path = [env!("CARGO_WORKSPACE_DIR"), "capnp_types", "src", "capnp"]
        .iter()
        .collect::<PathBuf>();

    // Clean up codegen output directory and module
    if output_dir_path.is_dir() {
        std::fs::remove_dir_all(&output_dir_path)?;
    }
    std::fs::create_dir_all(&output_dir_path)?;

    let mut master_capnp_module_path = output_dir_path.clone();
    master_capnp_module_path.set_extension("rs");
    if master_capnp_module_path.exists() {
        std::fs::remove_file(&master_capnp_module_path)?;
    }
    let mut parent_capnp_module = std::fs::File::create(&master_capnp_module_path)?;

    for glob_result in glob::glob(&format!(
        "{}/{}/[a-zA-Z]*.capnp",
        env!("CARGO_WORKSPACE_DIR"),
        capnp_schemas_dir_name,
    ))? {
        let capnp_file = glob_result?;

        println!(
            "Compiling {}",
            capnp_file
                .as_os_str()
                .to_str()
                .ok_or_else(|| std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "Could not make string for file from glob."
                ))?
        );
        let mut capnp_compiler = capnpc::CompilerCommand::new();
        capnp_compiler
            .output_path(&output_dir_path)
            .default_parent_module(vec![capnp_types_dir_name.to_string()])
            .src_prefix(&capnp_schemas_dir_name);
        capnp_compiler.file(&capnp_file);
        capnp_compiler.run().map_err(DynError::from)?;

        // Module name is the base name with . replaced with _
        let capnp_rust_module_name = capnp_file
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::Other, "Could not get basename")
            })?
            .replace(".", "_");
        // Expose generated rust code in parent module
        parent_capnp_module.write_all(b"#[allow(unused)]\n")?;
        parent_capnp_module
            .write_all(format!("pub mod {};\n", capnp_rust_module_name).as_bytes())?;
    }
    Ok(())
}
