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
use tracing::{debug, warn};

use crate::{
    components::{Component, ComponentStatus, InitializedComponent, PortBoundComponent},
    effect::{
        announcements::ControlAnnouncement, diagnostics_port::DumpConsensusStateRequest,
        EffectBuilder, EffectExt, Effects,
    },
    types::NodeRng,
    utils::umask,
    WithDir,
};
pub use tasks::FileSerializer;
use util::ShowUnixAddr;

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

/// Diagnostics port component.
#[derive(Debug, DataSize)]
pub(crate) struct DiagnosticsPort {
    status: ComponentStatus,
    /// Sender which will cause server and client connections to exit when dropped.
    #[data_size(skip)]
    #[allow(dead_code)] // only used for its `Drop` impl.
    shutdown_sender: Option<watch::Sender<()>>,
    config: WithDir<Config>,
}

impl DiagnosticsPort {
    /// Creates a new diagnostics port component.
    pub(crate) fn new(config: WithDir<Config>) -> Self {
        DiagnosticsPort {
            status: ComponentStatus::Uninitialized,
            config,
            shutdown_sender: None,
        }
    }
}

/// Diagnostics port event.
#[derive(Debug, Serialize)]
pub(crate) enum Event {
    Initialize,
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("diagnostics port event")
    }
}

/// A diagnostics port initialization error.
#[derive(Debug, Error)]
pub(crate) enum Error {
    /// Error setting up the diagnostics port's unix socket listener.
    #[error("could not setup diagnostics port listener")]
    SetupListener(#[from] io::Error),
}

impl<REv> Component<REv> for DiagnosticsPort
where
    REv: From<Event> + From<DumpConsensusStateRequest> + From<ControlAnnouncement> + Send,
{
    type Event = Event;
    type ConstructionError = Error;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Event> {
        match event {
            Event::Initialize => {
                if self.status != ComponentStatus::Uninitialized {
                    return Effects::new();
                }
                let (effects, status) = self.bind(self.config.value().enabled, effect_builder);
                self.status = status;
                effects
            }
        }
    }
}

impl<REv> InitializedComponent<REv> for DiagnosticsPort
where
    REv: From<Event> + From<DumpConsensusStateRequest> + From<ControlAnnouncement> + Send,
{
    fn status(&self) -> ComponentStatus {
        self.status.clone()
    }
}

impl<REv> PortBoundComponent<REv> for DiagnosticsPort
where
    REv: From<Event> + From<DumpConsensusStateRequest> + From<ControlAnnouncement> + Send,
{
    type Error = Error;
    type ComponentEvent = Event;

    fn listen(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<Effects<Event>, Self::Error> {
        let (shutdown_sender, shutdown_receiver) = watch::channel(());

        self.shutdown_sender = Some(shutdown_sender);

        let cfg = self.config.value();

        let socket_path = self.config.with_dir(cfg.socket_path.clone());
        let listener = setup_listener(
            &socket_path,
            // Mac OS X / Linux use different types for the mask, so we need to call .into() here.
            #[allow(clippy::useless_conversion)]
            cfg.socket_umask.into(),
        )?;
        let server = tasks::server(effect_builder, socket_path, listener, shutdown_receiver);
        Ok(server.ignore())
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
                // This happens if a background program races us for the removal, as it usually
                // means the file is already gone. We can ignore this, but make note of it in the
                // log.
                warn!(%err, "could not remove stale socket file, assuming race with other process");
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::prelude::{FileTypeExt, PermissionsExt},
    };

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::UnixStream,
    };

    use super::setup_listener;

    #[tokio::test]
    async fn setup_listener_creates_listener() {
        const TEST_MESSAGE: &[u8] = b"hello, world!";

        let tmpdir = tempfile::tempdir().expect("could not create tempdir");
        let socket_path = tmpdir.path().join("test.socket");

        // We give it a strict umask to check.
        let listener = setup_listener(&socket_path, 0o077).expect("could not setup listener");

        let meta = fs::metadata(&socket_path).expect("could not get metadata");
        // With the given umask, world and group permissions should be 0.
        assert_eq!(meta.permissions().mode() & 0o077, 0);

        // Attempt to connect.
        tokio::spawn(async move {
            let mut stream = UnixStream::connect(socket_path)
                .await
                .expect("could not connect to listener");
            stream
                .write_all(TEST_MESSAGE)
                .await
                .expect("could not write to listener");
        });

        let (mut stream, _socket_addr) = listener
            .accept()
            .await
            .expect("could not accept connection");

        let mut buffer = Vec::new();
        stream
            .read_to_end(&mut buffer)
            .await
            .expect("failed to read to end");
        assert_eq!(TEST_MESSAGE, buffer.as_slice());
    }

    #[tokio::test]
    async fn setup_listener_removes_previous_listener() {
        let tmpdir = tempfile::tempdir().expect("could not create tempdir");
        let socket_path = tmpdir.path().join("overwrite-me.socket");

        fs::write(&socket_path, b"this-file-should-be-deleted-soon")
            .expect("could not write to socket-blocking temporary file");

        let meta = fs::metadata(&socket_path).expect("could not get metadata");
        assert!(
            !meta.file_type().is_socket(),
            "temporary file created should not be a socket"
        );

        // Creating the listener should remove the underlying file.
        let _listener = setup_listener(&socket_path, 0o022).expect("could not setup listener");

        let meta = fs::metadata(&socket_path).expect("could not get metadata");
        assert!(
            meta.file_type().is_socket(),
            "did not overwrite previous file"
        );
    }
}
