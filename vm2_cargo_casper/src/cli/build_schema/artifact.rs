use std::{mem::MaybeUninit, path::Path};

use libloading::{Library, Symbol};

const COLLECT_SCHEMA_FUNC: &str = "__cargo_casper_collect_schema";

type CollectSchema = unsafe extern "C" fn(size_ptr: *mut u64) -> *mut u8;

pub(crate) struct Artifact {
    library: Library,
}

impl Artifact {
    pub(crate) fn from_path<P: AsRef<Path>>(
        artifact_path: P,
    ) -> Result<Artifact, libloading::Error> {
        let library = unsafe { libloading::Library::new(artifact_path.as_ref()) }?;

        Ok(Self { library })
    }

    /// Collects schema from the built artifact.
    ///
    /// This returns a [`serde_json::Value`] to skip validation of a `Schema` object structure which
    /// (in theory) can differ.
    pub(crate) fn collect_schema(&self) -> serde_json::Result<serde_json::Value> {
        let collect_schema: Symbol<CollectSchema> =
            unsafe { self.library.get(COLLECT_SCHEMA_FUNC.as_bytes()).unwrap() };

        let json_bytes = {
            let mut value = MaybeUninit::uninit();
            let leaked_json_bytes = unsafe { collect_schema(value.as_mut_ptr()) };
            let size = unsafe { value.assume_init() };
            let length: usize = size.try_into().unwrap();
            unsafe { Vec::from_raw_parts(leaked_json_bytes, length, length) }
        };

        serde_json::from_slice(&json_bytes)
    }
}
