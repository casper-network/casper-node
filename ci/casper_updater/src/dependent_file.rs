use std::{
    fs,
    path::{Path, PathBuf},
};

use regex::Regex;

/// A file which is dependent on the version of a certain CasperLabs crate.
pub struct DependentFile {
    /// Full path to the file.
    path: PathBuf,
    /// Current contents of the file.
    contents: String,
    /// Regex applicable to the portion to be updated.
    regex: Regex,
    /// Function which generates the replacement string once the updated version is known.
    replacement: fn(&str) -> String,
}

impl DependentFile {
    pub fn new<P: AsRef<Path>>(
        relative_path: P,
        regex: Regex,
        replacement: fn(&str) -> String,
    ) -> Self {
        let path = crate::root_dir().join(relative_path);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("should read {}: {:?}", path.display(), error));
        assert!(
            regex.find(&contents).is_some(),
            "regex '{}' failed to get a match in {}",
            regex,
            path.display()
        );

        DependentFile {
            path,
            contents,
            regex,
            replacement,
        }
    }

    pub fn update(&self, updated_version: &str) {
        let updated_contents = self
            .regex
            .replace(&self.contents, (self.replacement)(updated_version).as_str());
        fs::write(&self.path, updated_contents.as_ref())
            .unwrap_or_else(|error| panic!("should write {}: {:?}", self.path.display(), error));
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn relative_path(&self) -> &Path {
        self.path
            .strip_prefix(crate::root_dir())
            .expect("should strip prefix")
    }

    pub fn contents(&self) -> &str {
        &self.contents
    }
}
