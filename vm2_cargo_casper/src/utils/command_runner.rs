use std::{
    collections::VecDeque,
    fmt::{Display, Formatter},
    io::{self, BufRead, BufReader, Write},
    os::unix::process::ExitStatusExt,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
};

use atty::Stream;
use crossterm::{cursor, style, terminal, QueueableCommand};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Outcome {
    #[error("Input/Output error: {0}")]
    Io(#[from] io::Error),
    #[error("Subprocess exited with error code: {0}")]
    ErrorCode(i32),
    #[error("Subprocess terminated by signal: {0}")]
    Signal(i32),
}

#[derive(Debug)]
pub enum Line {
    Stdout(String),
    Stderr(String),
}

impl Display for Line {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Line::Stdout(text) => write!(f, "{}", text),
            Line::Stderr(text) => write!(f, "{}", text),
        }
    }
}

/// Maximum number of lines to keep in the rolling log.
pub const DEFAULT_MAX_LINES: usize = 10;

#[derive(Debug)]
pub struct ProcessHandle {
    pub receiver: mpsc::Receiver<Line>,
    pub stdout_thread_handle: thread::JoinHandle<()>,
    pub stderr_thread_handle: thread::JoinHandle<()>,
    pub child: std::process::Child,
}

impl ProcessHandle {
    pub fn wait(mut self) -> Result<(), Outcome> {
        // Ensure the reader threads have completed.
        self.stdout_thread_handle
            .join()
            .expect("Stdout thread panicked");
        self.stderr_thread_handle
            .join()
            .expect("Stderr thread panicked");

        // Wait for the subprocess to finish.
        let exit_status = self.child.wait().expect("Failed to wait on child process");

        match exit_status.code() {
            Some(code) => {
                if code == 0 {
                    // Subprocess completed successfully.
                    Ok(())
                } else {
                    // Subprocess exited with error code.
                    Err(Outcome::ErrorCode(code))
                }
            }
            None => {
                // Subprocess terminated by signal.
                if let Some(signal) = exit_status.signal() {
                    // Subprocess terminated by signal
                    Err(Outcome::Signal(signal))
                } else {
                    unreachable!("Unexpected exit status: {:?}", exit_status);
                }
            }
        }
    }
}

/// Runs a subprocess and captures its output.
///
/// Returns a `ProcessHandle` that can be used to read the output and wait for the process to
/// finish.
///
/// Lines captured are available in a `receiver` attribute and can be piped to a `LogTrail`
/// instance.
pub fn run_process(command: &mut Command) -> io::Result<ProcessHandle> {
    // Spawn the subprocess with stdout and stderr piped.
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    // Take the stdout and stderr handles.
    let stdout_pipe = child.stdout.take().expect("Failed to capture stdout");
    let stderr_pipe = child.stderr.take().expect("Failed to capture stderr");

    // Create a channel to receive lines from both stdout and stderr.
    let (tx, rx) = mpsc::channel();

    // Spawn a thread to read stdout.
    let stdout_thread = thread::spawn({
        let tx = tx.clone();

        move || {
            let reader = BufReader::new(stdout_pipe);
            for line in reader.lines() {
                if let Ok(line_text) = line {
                    // If send fails, the main thread is likely gone.
                    if tx.send(Line::Stdout(line_text)).is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    });

    // Spawn a second thread to read stderr.

    let stderr_thread = thread::spawn({
        let tx_err = tx.clone();
        move || {
            let reader = BufReader::new(stderr_pipe);
            for line in reader.lines() {
                if let Ok(line_text) = line {
                    if tx_err.send(Line::Stderr(line_text)).is_err() {
                        break;
                    }
                } else {
                    break;
                }
            }
        }
    });

    // Drop the extra sender so that the channel closes when both threads finish.
    drop(tx);

    Ok(ProcessHandle {
        receiver: rx,
        stdout_thread_handle: stdout_thread,
        stderr_thread_handle: stderr_thread,
        child,
    })
}

/// Enum representing the interactive mode for the log trail.
pub enum Interactive {
    /// Program will figure it out if a logs can be printed interactively.
    Auto,
    /// Interactive mode is enabled.
    Yes,
    /// Interactive mode is disabled.
    No,
}

impl Interactive {
    /// Check if the interactive mode is enabled.
    pub fn is_enabled(&self) -> bool {
        match self {
            Interactive::Auto => atty::is(Stream::Stdout),
            Interactive::Yes => true,
            Interactive::No => false,
        }
    }
}
/// A stateful log trail that maintains a rolling window of log lines.
pub struct LogTrail {
    max_lines: usize,
    interactive: Interactive,
    current_lines: VecDeque<String>,
    printed_lines: usize,
    stdout: std::io::Stdout,
}

impl LogTrail {
    /// Create a new LogTrail.
    ///
    /// * `max_lines` specifies how many lines to keep in the rolling window.
    /// * `interactive` should be true when you want the dynamic updating behavior (e.g. when
    ///   running in a terminal).
    pub fn new(max_lines: usize, interactive: Interactive) -> Self {
        Self {
            max_lines,
            interactive,
            current_lines: VecDeque::with_capacity(max_lines),
            printed_lines: 0,
            stdout: io::stdout(),
        }
    }

    /// Push a new line into the log trail.
    ///
    /// This method tracks the line numbering and either updates the dynamic window (if interactive)
    /// or prints the new line immediately.
    pub fn push_line<S: Into<String>>(&mut self, line: S) -> io::Result<()> {
        let line_text = line.into();
        if self.interactive.is_enabled() {
            // Maintain a rolling window of at most max_lines.
            if self.current_lines.len() == self.max_lines {
                self.current_lines.pop_front();
            }
            self.current_lines.push_back(line_text);
            // Move the cursor up by the number of previously printed lines plus one extra
            // (e.g. if a static header line is printed above the log).
            if self.printed_lines > 0 {
                self.stdout
                    .queue(cursor::MoveUp(self.printed_lines as u16))?;
            }
            // Clear everything from the current cursor position downward.
            self.stdout
                .queue(terminal::Clear(terminal::ClearType::FromCursorDown))?;

            // Reprint the rolling buffer with each line prefixed.
            for text in self.current_lines.iter() {
                self.stdout.queue(style::Print(text))?;
                self.stdout.queue(style::Print("\n"))?;
            }
            self.printed_lines = self.current_lines.len();
        } else {
            // In non-interactive mode simply print the line.
            self.stdout.queue(style::Print(line_text))?;
            self.stdout.queue(style::Print("\n"))?;
        }
        self.stdout.flush()?;
        Ok(())
    }
}

/// Builder for creating a `LogTrail` instance.
#[derive(Default)]
pub struct LogTrailBuilder {
    max_lines: Option<usize>,
    interactive: Option<Interactive>,
}

impl LogTrailBuilder {
    /// Creates a new builder with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum number of lines for the rolling log.
    pub fn max_lines(mut self, max_lines: usize) -> Self {
        self.max_lines = Some(max_lines);
        self
    }

    /// Sets whether the log trail should be interactive.
    pub fn interactive(mut self, interactive: Interactive) -> Self {
        self.interactive = Some(interactive);
        self
    }

    /// Builds the `LogTrail` instance.
    pub fn build(self) -> LogTrail {
        let max_lines = self.max_lines.expect("Max lines must be set");
        let interactive = self.interactive.expect("Interactive mode must be set");
        LogTrail::new(max_lines, interactive)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_process() {
        // This test will run the `echo` command, which should always succeed.
        let result = run_process(Command::new("echo").args(["Hello, world!"]))
            .expect("Failed to run process");
        assert!(result.wait().is_ok());
    }

    #[test]
    fn test_run_interactive_process() {
        // This test will run the `echo` command, which should always succeed.
        let result = run_process(Command::new("echo").args(["Hello, world!"]))
            .expect("Failed to run process");
        assert!(result.wait().is_ok());
    }

    #[test]
    fn test_run_process_failure() {
        // This test will run a non-existent command, which should fail.
        let result = run_process(&mut Command::new("non_existent_command"))
            .expect_err("Failed to run process");
        assert_eq!(result.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn test_run_process_with_env() {
        // This test will run the `env` command to print environment variables.
        let handle = run_process(Command::new("env").envs([("TEST_VAR", "test_value")]))
            .expect("Failed to run process");

        let captured_lines: Vec<String> = handle
            .receiver
            .into_iter()
            .map(|line| line.to_string())
            .collect();
        let output = captured_lines.join("\n");
        assert!(output.contains("TEST_VAR=test_value"));
    }
}
