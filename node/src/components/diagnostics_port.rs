//! Diagnostics port component.
//!
//! The diagnostics port listens on a configurable unix socket for incoming connections and allows
//! deep debug access to a running node via special commands.

mod command;
mod tasks;
mod util;

use std::{
    fmt::{self, Display, Formatter},
    fs, io,
    path::{Path, PathBuf},
};

use datasize::DataSize;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{net::UnixListener, sync::watch};
use tracing::debug;

use super::Component;
use crate::{
    effect::{
        announcements::ControlAnnouncement, diagnostics_port::DumpConsensusStateRequest,
        EffectBuilder, EffectExt, Effects,
    },
    reactor::EventQueueHandle,
    types::NodeRng,
    utils::umask,
    WithDir,
};
pub use tasks::TempFileSerializer;
use util::ShowUnixAddr;

/// Diagnostics port component.
#[derive(Debug, DataSize)]
pub(crate) struct DiagnosticsPort {
    /// Sender, when dropped, will cause server and client connections to exit.
    #[data_size(skip)]
    #[allow(dead_code)] // only used for its `Drop` impl.
    shutdown_sender: watch::Sender<()>,
}

/// Diagnostics port configuration.
#[derive(Clone, DataSize, Debug, Deserialize)]
pub(crate) struct Config {
    /// Whether or not the diagnostics port is enabled.
    enabled: bool,
    /// Path to listen on.
    socket_path: PathBuf,
    /// `umask` to apply before creating the socket.
    socket_umask: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            socket_path: "debug.socket".into(),
            socket_umask: 0o077,
        }
    }
}

impl DiagnosticsPort {
    /// Creates a new diagnostics port component.
    pub(crate) fn new<REv>(
        cfg: &WithDir<Config>,
        event_queue: EventQueueHandle<REv>,
    ) -> Result<(Self, Effects<Event>), Error>
    where
        REv: From<DumpConsensusStateRequest> + From<ControlAnnouncement> + Send,
    {
        let config = cfg.value();
        let (shutdown_sender, shutdown_receiver) = watch::channel(());

        if !config.enabled {
            // If not enabled, do not launch a background task, simply exit immediately.
            //
            // Having a shutdown sender around still is harmless.
            debug!("diagnostics port disabled");
            return Ok((DiagnosticsPort { shutdown_sender }, Effects::new()));
        }

        let socket_path = cfg.with_dir(config.socket_path.clone());
        let listener = setup_listener(&socket_path, config.socket_umask.into())?;
        let server = tasks::server(
            EffectBuilder::new(event_queue),
            socket_path,
            listener,
            shutdown_receiver,
        );

        Ok((DiagnosticsPort { shutdown_sender }, server.ignore()))
    }
}

/// Sets up a UNIX socket listener at the given path.
///
/// If the socket already exists, an attempt to delete it is made. Errors during deletion are
/// ignored, but may cause the subsequent socket opening to fail.
fn setup_listener<P: AsRef<Path>>(path: P, socket_umask: umask::Mode) -> io::Result<UnixListener> {
    let socket_path = path.as_ref();

    // This would be racy, but no one is racing us for the socket, so we'll just do a naive
    // check-then-delete :).
    if socket_path.exists() {
        debug!(socket_path=%socket_path.display(), "found stale socket file, trying to remove");
        match fs::remove_file(&socket_path) {
            Ok(_) => {
                debug!("stale socket file removed");
            }
            Err(err) => {
                // This happens if a background tasks races us for the removal, as it usually
                // means the file is already gone. We can ignore this, but make note of it in
                // the log.
                debug!(%err, "could not remove stale socket file, assuming race with background task");
            }
        }
    }

    // This is not thread-safe, as it will set the umask for the entire process, but we assume that
    // initalization happens "sufficiently single-threaded".
    let umask_guard = umask::temp_umask(socket_umask);
    let listener = UnixListener::bind(socket_path)?;
    drop(umask_guard);

    debug!(local_addr=%ShowUnixAddr(&listener.local_addr()?), "diagnostics port listening");

    Ok(listener)
}

/// Diagnostics port event.
#[derive(Debug, Serialize)]
pub(crate) struct Event;

/// A diagnostics port initialization error.
#[derive(Debug, Error)]
pub(crate) enum Error {
    /// Error setting up the diagnostics port's unix socket listener.
    #[error("could not setup diagnostics port listener")]
    SetupListener(#[from] io::Error),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("diagnostics port event")
    }
}

impl<REv> Component<REv> for DiagnosticsPort {
    type Event = Event;

    type ConstructionError = Error;

    fn handle_event(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        _event: Event,
    ) -> Effects<Event> {
        // No events are processed in the component, as all requests are handled per-client in
        // tasks.
        Effects::new()
    }
}
