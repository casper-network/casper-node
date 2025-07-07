use std::{
    ffi::OsStr,
    path::PathBuf,
    process::{Command, Stdio},
};

use anyhow::{anyhow, Result};

use crate::utils::command_runner::{self, DEFAULT_MAX_LINES};

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
            "--color=always",
            "--message-format=json-diagnostic-rendered-ansi",
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

        // Run the process and capture the output from both stdout and stderr.
        let handle = command_runner::run_process(&mut command)?;

        let mut log_trail = command_runner::LogTrailBuilder::new()
            .max_lines(DEFAULT_MAX_LINES)
            .interactive(command_runner::Interactive::Auto)
            .build();
        let mut artifacts = Vec::new();
        for line in &handle.receiver {
            match line {
                command_runner::Line::Stdout(line) => {
                    match serde_json::from_str::<cargo_metadata::Message>(&line.to_string())
                        .expect("Parse")
                    {
                        cargo_metadata::Message::CompilerArtifact(artifact) => {
                            for artifact in &artifact.filenames {
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
                        cargo_metadata::Message::CompilerMessage(compiler_message) => {
                            log_trail.push_line(compiler_message.to_string())?;
                        }
                        cargo_metadata::Message::BuildScriptExecuted(_build_script) => {}
                        cargo_metadata::Message::BuildFinished(_build_finished) => {}
                        cargo_metadata::Message::TextLine(text) => log_trail.push_line(text)?,
                        _ => todo!(),
                    }
                }
                command_runner::Line::Stderr(line) => {
                    log_trail.push_line(line)?;
                }
            }
        }

        match handle.wait() {
            Ok(()) => {
                // Process completed successfully.
            }
            Err(command_runner::Outcome::Io(error)) => {
                return Err(anyhow!("Cargo build failed with error code: {error}"));
            }
            Err(command_runner::Outcome::ErrorCode(code)) => {
                return Err(anyhow!("Cargo build failed with error code: {code}"));
            }
            Err(command_runner::Outcome::Signal(signal)) => {
                return Err(anyhow!("Cargo build was terminated by signal: {signal}"));
            }
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
