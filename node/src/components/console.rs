//! Unix console component.
//!
//! The console listens on a configurable unix socket for incoming connections and allows deep debug
//! access to a running node via special commands.

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
    effect::{EffectBuilder, EffectExt, Effects},
    types::NodeRng,
    WithDir,
};
use util::ShowUnixAddr;

mod command;
mod tasks;
mod util;

/// Unix console component.
#[derive(Debug, DataSize)]
pub(crate) struct Console {
    /// Sender, when dropped, will cause server and client connections to exit.
    #[data_size(skip)]
    shutdown_sender: watch::Sender<()>,
}

/// Unix console configuration.
#[derive(Clone, DataSize, Debug, Default, Deserialize)]
pub(crate) struct Config {
    /// Whether or not the console is enabled.
    enabled: bool,
    /// Path to listen on.
    socket_path: PathBuf,
}

impl Console {
    /// Creates a new Unix console component.
    pub(crate) fn new(cfg: &WithDir<Config>) -> Result<(Self, Effects<Event>), Error> {
        let config = cfg.value();
        let (shutdown_sender, shutdown_receiver) = watch::channel(());

        if !config.enabled {
            // If not enabled, do not launch a background task, simply exit immediately.
            //
            // Having a shutdown sender around still is harmless.
            return Ok((Console { shutdown_sender }, Effects::new()));
        }

        let socket_path = cfg.with_dir(config.socket_path.clone());
        let listener = setup_listener(&socket_path)?;
        let server = tasks::server(socket_path, listener, shutdown_receiver);

        Ok((Console { shutdown_sender }, server.ignore()))
    }
}

/// Sets up a UNIX socket listener at the given path.
///
/// If the socket already exists, an attempt to delete it is made. Errors during deletion are
/// ignored, but may cause the subsequent socket opening to fail.
fn setup_listener<P: AsRef<Path>>(path: P) -> io::Result<UnixListener> {
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

    let listener = UnixListener::bind(socket_path)?;
    debug!(local_addr=%ShowUnixAddr(&listener.local_addr()?), "console socket listening");

    Ok(listener)
}

/// Unix console event.
#[derive(Debug, Serialize)]
pub(crate) struct Event;

/// A console initialization error.
#[derive(Debug, Error)]
pub(crate) enum Error {
    /// Error setting up the console's unix socket listener.
    #[error("could not setup console listener")]
    SetupListener(#[from] io::Error),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("console event")
    }
}

impl<REv> Component<REv> for Console {
    type Event = Event;

    type ConstructionError = Error;

    fn handle_event(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        _event: Event,
    ) -> Effects<Event> {
        // No events are processed in the component, as all requests are handled per-client in tasks.
        Effects::new()
    }
}
