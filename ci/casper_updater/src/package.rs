use std::{
    convert::TryFrom,
    io::{self, Write},
    path::Path,
};

use regex::Regex;

use casper_types::SemVer;

use crate::{
    dependent_file::DependentFile,
    regex_data::{
        MANIFEST_NAME_REGEX, MANIFEST_VERSION_REGEX, PACKAGE_JSON_NAME_REGEX,
        PACKAGE_JSON_VERSION_REGEX,
    },
};

const CAPTURE_INDEX: usize = 2;

/// Represents a published CasperLabs crate or AssemblyScript package which may need its version
/// updated.
pub struct Package {
    /// This package's name as specified in its manifest.
    name: String,
    /// This package's current version as specified in its manifest.
    current_version: SemVer,
    /// Files which must be updated if this package's version is changed, including this package's
    /// own manifest file.  The other files will often be from a different package.
    dependent_files: &'static Vec<DependentFile>,
}

trait PackageConsts {
    const MANIFEST: &'static str;
    fn name_regex() -> &'static Regex;
    fn version_regex() -> &'static Regex;
}

struct CargoPackage;

impl PackageConsts for CargoPackage {
    const MANIFEST: &'static str = "Cargo.toml";

    fn name_regex() -> &'static Regex {
        &*MANIFEST_NAME_REGEX
    }

    fn version_regex() -> &'static Regex {
        &*MANIFEST_VERSION_REGEX
    }
}

struct AssemblyScriptPackage;

impl PackageConsts for AssemblyScriptPackage {
    const MANIFEST: &'static str = "package.json";

    fn name_regex() -> &'static Regex {
        &*PACKAGE_JSON_NAME_REGEX
    }

    fn version_regex() -> &'static Regex {
        &*PACKAGE_JSON_VERSION_REGEX
    }
}

#[allow(clippy::ptr_arg)]
impl Package {
    pub fn cargo<P: AsRef<Path>>(
        relative_path: P,
        dependent_files: &'static Vec<DependentFile>,
    ) -> Self {
        Self::new::<_, CargoPackage>(relative_path, dependent_files)
    }

    pub fn assembly_script<P: AsRef<Path>>(
        relative_path: P,
        dependent_files: &'static Vec<DependentFile>,
    ) -> Self {
        Self::new::<_, AssemblyScriptPackage>(relative_path, dependent_files)
    }

    fn new<P: AsRef<Path>, T: PackageConsts>(
        relative_path: P,
        dependent_files: &'static Vec<DependentFile>,
    ) -> Self {
        let manifest_path = crate::root_dir().join(&relative_path).join(T::MANIFEST);

        let manifest = dependent_files
            .iter()
            .find(|&file| file.path() == manifest_path)
            .unwrap_or_else(|| {
                panic!(
                    "{} should be a dependent file of {}",
                    manifest_path.display(),
                    relative_path.as_ref().display()
                )
            });

        let find_value = |regex: &Regex| {
            regex
                .captures(manifest.contents())
                .unwrap_or_else(|| {
                    panic!(
                        "should find package name and version in {}",
                        manifest_path.display()
                    )
                })
                .get(CAPTURE_INDEX)
                .unwrap_or_else(|| {
                    panic!(
                        "package name and version should be regex capture at index {} in {}",
                        CAPTURE_INDEX,
                        manifest_path.display()
                    )
                })
                .as_str()
                .to_string()
        };

        let name = find_value(T::name_regex());
        let version = find_value(T::version_regex());
        let current_version = SemVer::try_from(&*version).expect("should parse current version");

        Package {
            name,
            current_version,
            dependent_files,
        }
    }

    pub fn update(&self) {
        if crate::is_dry_run() {
            println!(
                "Current version of {} is {}",
                self.name, self.current_version
            );
            if let Some(bump_version) = crate::bump_version() {
                let updated_version = bump_version.update(self.current_version);
                println!("Will be updated to {}", updated_version);
            }
            println!("Files affected by this package's version:");
            for dependent_file in self.dependent_files {
                println!("\t* {}", dependent_file.relative_path().display());
            }
            println!();
            return;
        }

        let updated_version = match crate::bump_version() {
            None => match get_updated_version_from_user(&self.name, self.current_version) {
                Some(version) => version,
                None => return,
            },
            Some(bump_version) => bump_version.update(self.current_version),
        };

        for dependent_file in self.dependent_files {
            dependent_file.update(&updated_version.to_string());
        }

        println!(
            "Updated {} from {} to {}.",
            self.name, self.current_version, updated_version
        );
    }
}

pub fn get_updated_version_from_user(name: &str, current_version: SemVer) -> Option<SemVer> {
    loop {
        print!(
            "Current {} version is {}.  Enter new version (leave blank for unchanged): ",
            name, current_version
        );
        io::stdout().flush().expect("should flush stdout");
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(_) => {
                input = input.trim_end().to_string();
                if input.is_empty() {
                    return None;
                }

                let new_version = match SemVer::try_from(&*input) {
                    Ok(version) => version,
                    Err(error) => {
                        println!("\n{} is not a valid version: {}.", input, error);
                        continue;
                    }
                };

                if new_version < current_version {
                    println!(
                        "Updated version ({}) is lower than current version ({})",
                        new_version, current_version
                    );
                    if crate::allow_earlier_version() {
                        println!("Allowing earlier version due to flag.")
                    } else {
                        continue;
                    }
                }

                return if new_version == current_version {
                    None
                } else {
                    Some(new_version)
                };
            }
            Err(error) => println!("\nFailed to read from stdin: {}.", error),
        }
    }
}
