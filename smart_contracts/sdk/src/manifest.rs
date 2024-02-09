/// Given contract defines a set of entry points.
pub trait ToManifest {
    fn to_manifest() -> &'static [casper_sdk_sys::EntryPoint];
}
