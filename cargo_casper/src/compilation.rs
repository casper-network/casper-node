use std::{
    ffi::OsStr,
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(tag = "reason")]
enum CargoMessage {
    #[serde(rename = "compiler-artifact")]
    CompilerArtifact { filenames: Vec<String> },
    #[serde(other)]
    Other,
}

/// Represents a job to compile a Cargo project.
pub(crate) struct CompileJob<'a> {
    package_name: Option<&'a str>,
    features: Vec<String>,
    env_vars: Vec<(&'a str, &'a str)>,
    in_dir: Option<PathBuf>,
}

impl<'a> CompileJob<'a> {
    /// Creates a new compile job with the given manifest path, optional features,
    /// and environmental variables.
    pub fn new(
        package_name: Option<&'a str>,
        features: Option<Vec<String>>,
        env_vars: Vec<(&'a str, &'a str)>,
    ) -> Self {
        Self {
            package_name,
            features: features.unwrap_or_default(),
            env_vars,
            in_dir: None,
        }
    }

    /// Dispatches the compilation job. This builds the Cargo project into a temporary target
    /// directory.
    pub fn dispatch<T, I, S>(&self, target: T, extra_features: I) -> Result<CompilationResults>
    where
        T: Into<String>,
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let target: String = target.into();

        // Merge the configured features with any extra features
        let mut features = self.features.clone();
        features.extend(extra_features.into_iter().map(Into::into));
        let features_str = features.join(",");

        let mut build_args = vec!["build"];

        if let Some(package_name) = self.package_name {
            build_args.push("-p");
            build_args.push(package_name);
        }

        build_args.extend_from_slice(&[
            "--target",
            target.as_str(),
            "--features",
            &features_str,
            "--lib",
            "--release",
            "--message-format=json",
        ]);

        // Run the cargo build command and capture the output
        let mut command = Command::new("cargo");
        command.args(&build_args);
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        for (key, value) in &self.env_vars {
            command.env(key, value);
        }

        if let Some(in_directory) = &self.in_dir {
            command.current_dir(in_directory);
        }

        let mut handle = command.spawn().context("Failed spawning child process")?;

        let stdout = handle.stdout.take().unwrap();
        let reader = BufReader::new(stdout);

        let mut artifacts = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if let Ok(CargoMessage::CompilerArtifact { filenames }) =
                serde_json::from_str::<CargoMessage>(&line)
            {
                for artifact in &filenames {
                    let path = PathBuf::from(artifact);
                    if path
                        .parent()
                        .and_then(|p| p.file_name())
                        .and_then(OsStr::to_str)
                        != Some("deps")
                    {
                        artifacts.push(PathBuf::from(artifact));
                    }
                }
            }
        }

        let output = handle
            .wait_with_output()
            .context("Failed compiling user wasm")?;

        if !output.status.success() {
            return Err(anyhow!(
                "Cargo build failed:\nSTDERR: {}\nSTDOUT: {}",
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            ));
        }

        Ok(CompilationResults { artifacts })
    }
}

/// Results of a compilation job.
pub(crate) struct CompilationResults {
    artifacts: Vec<PathBuf>,
}

impl CompilationResults {
    /// Returns a slice of paths to the build artifacts.
    pub fn artifacts(&self) -> &[PathBuf] {
        &self.artifacts
    }

    pub fn get_artifact_by_extension(&self, extension: &str) -> Option<PathBuf> {
        self.artifacts()
            .iter()
            .find(|x| x.extension().and_then(|y| y.to_str()) == Some(extension))
            .map(|x| x.into())
    }
}
