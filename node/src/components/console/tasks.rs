use std::{
    borrow::Cow,
    fmt::{self, Debug, Display, Formatter},
    fs, io,
    path::PathBuf,
};

use crate::{
    components::consensus::EraDump,
    effect::{
        announcements::ControlAnnouncement, console::DumpConsensusStateRequest, EffectBuilder,
    },
};

use super::{
    command::{Action, Command, OutputFormat},
    util::ShowUnixAddr,
};
use casper_types::EraId;
use futures::future::{self, Either};
use serde::Serialize;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{unix::OwnedWriteHalf, UnixListener, UnixStream},
    sync::watch,
};
use tracing::{debug, info, info_span, warn, Instrument};

/// Success or failure response.
///
/// This response is sent back to clients after every operation (unless suppressed in quiet mode),
/// indicating the outcome of the operation.
#[derive(Debug, Serialize)]
enum Outcome {
    /// Operation succeeded.
    Success {
        /// Human-readable message giving additional info and/or stating the effect.
        msg: String,
    },
    /// Operation failed.
    Failure {
        /// Human-readable message describing the failure that occurred.
        reason: String,
    },
}

impl Outcome {
    /// Constructs a new successful outcome.
    fn success<S: ToString>(msg: S) -> Self {
        Outcome::Success {
            msg: msg.to_string(),
        }
    }

    /// Constructs a new failed outcome.
    fn failed<S: ToString>(reason: S) -> Self {
        Outcome::Failure {
            reason: reason.to_string(),
        }
    }
}

impl Display for Outcome {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::Success { msg } => {
                write!(f, "OK   {}", msg)
            }
            Outcome::Failure { reason } => {
                write!(f, "ERR  {}", reason)
            }
        }
    }
}

/// Configuration for a connection console session.
#[derive(Copy, Clone, Debug, Default, Serialize)]
struct Session {
    /// Whether or not to suppress the operation outcome.
    quiet: bool,
    /// Output format to send to client.
    output: OutputFormat,
}

impl Display for Session {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

impl Session {
    /// Creates a serializer for an `EraDump`.
    fn create_era_dump_serializer(&self) -> fn(&EraDump<'_>) -> Result<Vec<u8>, Cow<'static, str>> {
        // TODO: This function could probably be a generic serialization function for any `T`, but
        // the conversion is tricky due to the lifetime arguments on `EraDump` and has not been done
        // yet.
        match self.output {
            OutputFormat::Interactive => |data: &EraDump| {
                let mut buf = data.to_string().into_bytes();
                buf.push(b'\n');
                Ok(buf)
            },
            OutputFormat::Json => |data: &EraDump| {
                let mut buf = serde_json::to_vec(&data).map_err(|err| {
                    Cow::Owned(format!("failed to serialize era dump as JSON: {}", err))
                })?;
                buf.push(b'\n');
                Ok(buf)
            },
            OutputFormat::Bincode => |data: &EraDump| {
                bincode::serialize(&data).map_err(|err| {
                    Cow::Owned(format!("failed to serialize era dump as bincode: {}", err))
                })
            },
        }
    }

    /// Processes a single command line sent from a client.
    async fn process_line<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        writer: &mut OwnedWriteHalf,
        line: &str,
    ) -> io::Result<()>
    where
        REv: From<DumpConsensusStateRequest> + From<ControlAnnouncement> + Send,
    {
        debug!(%line, "line received");
        match Command::from_line(line) {
            Ok(ref cmd) => {
                info!(?cmd, "processing command");
                match cmd.action {
                    Action::Session => {
                        self.send_outcome(writer, &Outcome::success("showing session info"))
                            .await?;
                        self.send_to_client(writer, &self).await?;
                    }
                    Action::Set { quiet, output } => {
                        let mut changed = false;

                        if let Some(quiet) = quiet {
                            changed |= self.quiet != quiet;
                            self.quiet = quiet;
                        }

                        if let Some(output) = output {
                            changed |= self.output != output;
                            self.output = output;
                        }

                        if changed {
                            self.send_outcome(writer, &Outcome::success("session updated"))
                                .await?;
                        } else {
                            self.send_outcome(writer, &Outcome::success("session unchanged"))
                                .await?;
                        }
                    }
                    Action::DumpConsensus { era } => {
                        let output = effect_builder
                            .console_dump_consensus_state(
                                era.map(EraId::new),
                                self.create_era_dump_serializer(),
                            )
                            .await;

                        match output {
                            Ok(ref data) => {
                                self.send_outcome(
                                    writer,
                                    &Outcome::success("dumping consensus state"),
                                )
                                .await?;
                                writer.write_all(data).await?;
                            }
                            Err(err) => {
                                self.send_outcome(writer, &Outcome::failed(err)).await?;
                            }
                        }
                    }
                    Action::DumpQueues => {
                        // TODO: Create serializer.
                        effect_builder.console_dump_queue().await;
                    }
                };
            }
            Err(err) => {
                self.send_outcome(writer, &Outcome::failed(err.to_string().as_str()))
                    .await?
            }
        }

        Ok(())
    }

    /// Sends an operation outcome.
    ///
    /// The outcome will be silently dropped if the session is in quiet mode.
    async fn send_outcome(
        &self,
        writer: &mut OwnedWriteHalf,
        response: &Outcome,
    ) -> io::Result<()> {
        if self.quiet {
            return Ok(());
        }

        self.send_to_client(writer, response).await
    }

    /// Sends a message to the client.
    ///
    /// Any type of message can be sent to a client, as long as it has a `Display` (use for
    /// `interactive` encoding) and `Serialize` (used for `bincode` and `json`) implementation.
    async fn send_to_client<T>(&self, writer: &mut OwnedWriteHalf, response: &T) -> io::Result<()>
    where
        T: Display + Serialize,
    {
        match self.output {
            OutputFormat::Interactive => {
                writer.write_all(response.to_string().as_bytes()).await?;
                writer.write_all(b"\n").await?;
            }
            OutputFormat::Json => {
                let buf = serde_json::to_string_pretty(response)
                    .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
                writer.write_all(buf.as_bytes()).await?;
                writer.write_all(b"\n").await?;
            }
            OutputFormat::Bincode => {
                let buf = bincode::serialize(response)
                    .map_err(|err| io::Error::new(io::ErrorKind::Other, err))?;
                writer.write_all(&buf).await?;
            }
        }

        Ok(())
    }
}

/// Handler for client connection.
///
/// The core loop for the console; reads commands via unix socket and processes them accordingly.
///
/// # Security
///
/// The handler itself will buffer an unlimited amount of data if no newline is encountered in the
/// input stream. For this reason ensure that only trusted client connect to the socket producing
/// the passed in `stream`.
async fn handler<REv>(
    effect_builder: EffectBuilder<REv>,
    stream: UnixStream,
    mut shutdown_receiver: watch::Receiver<()>,
) -> io::Result<()>
where
    REv: From<DumpConsensusStateRequest> + From<ControlAnnouncement> + Send,
{
    debug!("accepted new connection on console socket");

    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    let mut session = Session::default();

    loop {
        let shutdown_messages = async { while shutdown_receiver.changed().await.is_ok() {} };

        match future::select(Box::pin(shutdown_messages), Box::pin(lines.next_line())).await {
            Either::Left(_) => {
                info!("shutting down console connection to client");
                break Ok(());
            }
            Either::Right((line_result, _)) => {
                if let Some(line) = line_result? {
                    session
                        .process_line(effect_builder, &mut writer, line.as_str())
                        .await?;
                } else {
                    info!("client closed console connection");
                    break Ok(());
                }
            }
        }
    }
}

/// Server task for console.
pub(super) async fn server<REv>(
    effect_builder: EffectBuilder<REv>,
    socket_path: PathBuf,
    listener: UnixListener,
    mut shutdown_receiver: watch::Receiver<()>,
) where
    REv: From<DumpConsensusStateRequest> + From<ControlAnnouncement> + Send,
{
    let handling_shutdown_receiver = shutdown_receiver.clone();
    let mut next_client_id: u64 = 0;
    let accept_connections = async move {
        loop {
            match listener.accept().await {
                Ok((stream, client_addr)) => {
                    let client_id = next_client_id;

                    let span = info_span!("console", client_id,);

                    span.in_scope(|| {
                        info!(client_addr = %ShowUnixAddr(&client_addr), "accepted connection");
                    });

                    next_client_id += 1;

                    tokio::spawn(
                        handler(effect_builder, stream, handling_shutdown_receiver.clone())
                            .instrument(span),
                    );
                }
                Err(err) => {
                    info!(%err, "failed to accept incoming connection on console socket");
                }
            }
        }
    };

    let shutdown_messages = async move { while shutdown_receiver.changed().await.is_ok() {} };

    // Now we can wait for either the `shutdown` channel's remote end to do be dropped or the
    // infinite loop to terminate, which never happens.
    match future::select(Box::pin(shutdown_messages), Box::pin(accept_connections)).await {
        Either::Left(_) => info!("shutting down console socket"),
        Either::Right(_) => unreachable!("server accept returns `!`"),
    }

    // When we're shutting down, we try to delete the socket, but only warn in case of failure.
    match fs::remove_file(&socket_path) {
        Ok(_) => {
            debug!(socket_path=%socket_path.display(), "removed socket file");
        }
        Err(_) => {
            warn!(socket_path=%socket_path.display(), "could not remove socket file");
        }
    }
}
