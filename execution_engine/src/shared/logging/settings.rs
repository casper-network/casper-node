use log::LevelFilter;

/// Settings used to initialize the global logger.
#[derive(Clone, Copy, Debug)]
pub struct Settings {
    max_level: LevelFilter,
    enable_metrics: bool,
    style: Style,
}

impl Settings {
    /// Constructs new `Settings`, where `max_level` sets the verbosity level above which messages
    /// will be filtered out.
    ///
    /// `Off` is the lowest level, through `Error`, `Warn`, `Info`, `Debug` to `Trace` at the
    /// highest level.
    ///
    /// By default, logging of metrics is disabled (see
    /// [`with_metrics_enabled()`](Settings::with_metrics_enabled)), and the logging-style is set
    /// to [`Style::Structured`].
    pub fn new(max_level: LevelFilter) -> Self {
        Settings {
            max_level,
            enable_metrics: false,
            style: Style::Structured,
        }
    }

    /// If `true`, log messages created via `log_metric()` and
    /// `log_duration()` are logged, regardless of the log-level.
    #[must_use]
    pub fn with_metrics_enabled(mut self, value: bool) -> Self {
        self.enable_metrics = value;
        self
    }

    /// Sets the logging style to structured or human-readable.
    #[must_use]
    pub fn with_style(mut self, value: Style) -> Self {
        self.style = value;
        self
    }

    pub(crate) fn max_level(&self) -> LevelFilter {
        self.max_level
    }

    pub(crate) fn enable_metrics(&self) -> bool {
        self.enable_metrics
    }

    pub(crate) fn style(&self) -> Style {
        self.style
    }
}

/// The style of generated log messages.
#[derive(Clone, Copy, Debug)]
pub enum Style {
    /// Hybrid structured log-messages, with a human-readable component followed by JSON formatted
    /// details.
    Structured,
    /// Human-readable log-messages.
    HumanReadable,
}
