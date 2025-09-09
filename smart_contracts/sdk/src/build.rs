use std::env;

/// Build-time configuration for emitting linker arguments.
///
/// Usage (in a build script):
/// BuildConfig::from_env().emit();
pub struct BuildConfig {
    target: String,
    /// When true, emit `--import-memory` for wasm32 targets.
    import_memory: bool,
    /// When true, emit `--export-table` for wasm32 targets.
    export_table: bool,
}

impl BuildConfig {
    /// Create a config using environment (TARGET) and sensible defaults:
    /// - If TARGET is wasm32-unknown-unknown, both flags default to true.
    /// - Otherwise, both flags default to false.
    pub fn from_env() -> Self {
        let target = env::var("TARGET").unwrap_or_default();
        let is_wasm = target == "wasm32-unknown-unknown";
        Self {
            target,
            import_memory: is_wasm,
            export_table: is_wasm,
        }
    }

    /// Create a config with an explicit target triple.
    pub fn new(target: impl Into<String>) -> Self {
        let target = target.into();
        let is_wasm = target == "wasm32-unknown-unknown";
        Self {
            target,
            import_memory: is_wasm,
            export_table: is_wasm,
        }
    }

    /// Override the target triple.
    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = target.into();
        self
    }

    /// Enable or disable `--import-memory` emission for wasm.
    pub fn with_import_memory(mut self, enable: bool) -> Self {
        self.import_memory = enable;
        self
    }

    /// Enable or disable `--export-table` emission for wasm.
    pub fn with_export_table(mut self, enable: bool) -> Self {
        self.export_table = enable;
        self
    }

    fn is_wasm32(&self) -> bool {
        self.target == "wasm32-unknown-unknown"
    }

    /// Emit configured Cargo link args when appropriate.
    pub fn emit(&self) {
        for arg in self.link_args() {
            println!("cargo:rustc-link-arg={arg}");
        }
    }

    /// Return the list of linker args that would be emitted.
    pub fn link_args(&self) -> Vec<&'static str> {
        if !self.is_wasm32() {
            return Vec::new();
        }
        let mut out = Vec::new();
        if self.import_memory {
            out.push("--import-memory");
        }
        if self.export_table {
            out.push("--export-table");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::BuildConfig;

    #[test]
    fn defaults_for_wasm_emit_both() {
        let cfg = BuildConfig::new("wasm32-unknown-unknown");
        assert_eq!(cfg.link_args(), vec!["--import-memory", "--export-table"]);
    }

    #[test]
    fn defaults_for_native_emit_none() {
        let cfg = BuildConfig::new("x86_64-apple-darwin");
        assert!(cfg.link_args().is_empty());
    }

    #[test]
    fn override_disable_import_memory() {
        let cfg = BuildConfig::new("wasm32-unknown-unknown").with_import_memory(false);
        assert_eq!(cfg.link_args(), vec!["--export-table"]);
    }

    #[test]
    fn override_disable_export_table() {
        let cfg = BuildConfig::new("wasm32-unknown-unknown").with_export_table(false);
        assert_eq!(cfg.link_args(), vec!["--import-memory"]);
    }

    #[test]
    fn changing_target_affects_output() {
        let cfg = BuildConfig::new("wasm32-unknown-unknown").with_target("aarch64-apple-darwin");
        assert!(cfg.link_args().is_empty());
    }
}
