use std::{ffi::c_void, path::PathBuf, ptr::NonNull};

use casper_sdk::abi_generator::ABI_COLLECTORS;
use clap::{Args, Parser, Subcommand};

#[derive(Debug, Subcommand)]
pub enum Command {
    Build {
        #[command(flatten)]
        manifest: clap_cargo::Manifest,
        #[command(flatten)]
        workspace: clap_cargo::Workspace,
        #[command(flatten)]
        features: clap_cargo::Features,
    },
    Symbols {
        filename: PathBuf,
    },
}
// ...
#[derive(Debug, clap::Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

type CasperLoadEntrypoints = unsafe extern "C" fn(
    unsafe extern "C" fn(*const casper_sdk::schema::SchemaEntryPoint, usize, *mut c_void),
    *mut c_void,
);
type CollectABI = unsafe extern "C" fn(*mut casper_sdk::abi::Definitions);

unsafe extern "C" fn load_entrypoints_cb(
    entrypoint: *const casper_sdk::schema::SchemaEntryPoint,
    count: usize,
    ctx: *mut c_void,
) {
    let slice = unsafe { std::slice::from_raw_parts(entrypoint, count) };
    // let ctx = NonNull::new(ctx as *mut Vec<casper_sdk::abi_generator::EntryPoint>).unwrap();
    // let ctx = unsafe { ctx.as_mut() };
    // ctx.extend(slice);
    dbg!(&slice);
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Build {
            manifest,
            workspace,
            features,
        } => {
            todo!("Build")
        }
        Command::Symbols { filename } => {
            let lib = unsafe { libloading::Library::new(&filename).unwrap() };

            let load_entrypoints: libloading::Symbol<CasperLoadEntrypoints> =
                unsafe { lib.get(b"__cargo_casper_load_entrypoints").unwrap() };
            let collect_abi: libloading::Symbol<CollectABI> =
                unsafe { lib.get(b"__cargo_casper_collect_abi").unwrap() };

            // let mut ptr: *mut *const casper_sdk_sys::EntryPoint = std::ptr::null_mut();
            let entrypoints = {
                let mut entrypoints: Vec<casper_sdk::abi_generator::EntryPoint> = Vec::new();
                let ctx = NonNull::from(&mut entrypoints);
                unsafe { load_entrypoints(load_entrypoints_cb, ctx.as_ptr() as _) }
            };

            let defs = {
                let mut defs = casper_sdk::abi::Definitions::default();
                let ptr = NonNull::from(&mut defs);
                unsafe {
                    collect_abi(ptr.as_ptr());
                }
                defs
            };
            dbg!(&defs);
        }
    }

    // let app = App::parse();
    // match app.casper {
    //     Opts::Casper(command) => {
    //         // match command {
    //         //     Command::Build { manifest_path } => {
    //         //         println!("Building contract from manifest: {:?}", manifest_path);
    //         //     }
    //         // }
    //         dbg!(&command);
    //     }
    // }
}
