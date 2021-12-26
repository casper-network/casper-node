//! Unix console component.
//!
//! The console listens on a configurable unix socket for incoming connections and allows deep debug
//! access to a running node via special commands.

use std::{fs, io, path::PathBuf};

use datasize::DataSize;
use serde::Deserialize;
use tokio::{net::UnixListener, sync::watch};
use tracing::{debug, warn};

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
struct Console {
    /// Sender, when dropped, will cause server and client connections to exit.
    #[data_size(skip)]
    shutdown_sender: watch::Sender<()>,
}

/// Unix console configuration.
#[derive(Debug, Deserialize)]
struct Config {
    /// Whether or not the console is enabled.
    enabled: bool,
    /// Path to listen on.
    socket_path: PathBuf,
}

impl Console {
    /// Creates a new Unix console component.
    fn new(cfg: &WithDir<Config>) -> io::Result<(Self, Effects<Event>)> {
        let config = cfg.value();
        let (shutdown_sender, shutdown_receiver) = watch::channel(());
        let socket_path = cfg.with_dir(config.socket_path.clone());

        if !config.enabled {
            // If not enabled, do not launch a background task, simply exit immediately.
            //
            // Having a shutdown sender around still is harmless.
            return Ok((Console { shutdown_sender }, Effects::new()));
        }

        // This would be racy, but no one is racing us for the socket, so we'll just do a naive
        // check-then-delete :).
        if socket_path.exists() {
            debug!(socket_path=%socket_path.display(), "found stale socket file, trying to remove");
            match fs::remove_file(&socket_path) {
                Ok(_) => {
                    debug!("stale socket file removed");
                },
                Err(err) => {
                    // This happens if a background tasks races us for the removal, as it usually
                    // means the file is already gone. We can ignore this, but make note of it in
                    // the log.
                    debug!(%err, "could not remove stale socket file, assuming race with background task");
                },
            }
        }

        let listener = UnixListener::bind(socket_path.clone())?;
        debug!(local_addr=%ShowUnixAddr(&listener.local_addr()?), "console socket listening");

        let server = tasks::server(socket_path, listener, shutdown_receiver);

        Ok((Console { shutdown_sender }, server.ignore()))
    }
}

/// Unix console event.
type Event = ();

impl<REv> Component<REv> for Console {
    type Event = ();

    type ConstructionError = io::Error;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Event> {
        // No events are processed in the component, as all requests are handled per-client in tasks.
        Effects::new()
    }
}
