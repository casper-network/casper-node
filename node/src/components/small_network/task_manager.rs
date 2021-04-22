//! A manager for background tasks.
//!
//! Long-running background tasks that need to be shutdown are captured/managed by the task manager,
//! which allows for waiting for all of them to complete before continuing.
//!
//! ```
//! let mut task_manager = TaskManager::new();
//!
//! task_manager.spawn(|shutdown| {
//!     for _ in 0..10 {
//!         tokio::select! {
//!             _ = &mut shutdown => {
//!                 // Shutdown received, abort.
//!                 break;
//!             }
//!
//!             // TODO better example?
//!         }
//!     }
//! });
//! ```

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use futures::{stream::FuturesUnordered, Future, StreamExt};
use tokio::{
    sync::watch,
    task::{JoinError, JoinHandle},
    time,
};
use tracing::{error, trace, warn};

/// A shutdown receiver.
///
/// Can be asked for a shutdown signal via `wait_for_shutdown`.
#[derive(Debug, Clone)]
struct ShutdownReceiver(watch::Receiver<()>);

impl ShutdownReceiver {
    /// Waits for the shutdown signal.
    ///
    /// The returned future is safe to cancel.
    pub(super) async fn wait_for_shutdown(&mut self) {
        match self.0.changed().await {
            Ok(()) => {
                // We are using the dropping of the channel as a signal, not the channel itself!
                error!("received value on shutdown channel, this should never happen")
            }
            Err(_recv_err) => {
                // All good.
            }
        }
    }
}

/// The outcome of awaiting a task for shutdown.
#[derive(Debug)]
enum ShutdownOutcome {
    /// The task was shut down completely.
    Ok,
    /// Could not join the task, it either panicked or was explicitly cancelled.
    JoinFailure(JoinError),
    /// Join timeout.
    ///
    /// The task is still running in the background, the join handle has been lost.
    TimedOut,
}

/// Manager for tasks.
///
/// Can be used to signal a group of tasks to shutdown and wait for their completion, optionally
/// cancelling execution. All operations are "best effort", won't panic even on poisoned locks.
///
/// Dropping a task manager will instruct all tasks to shutdown, but the `drop` will not await their
/// termination.
#[derive(Debug)]
struct TaskManager {
    /// A counter for joined tasks.
    ///
    /// Incremented to provide unique IDs for tasks.
    task_counter: usize,
    /// Join handles for all spawned tasks, mapped from task ID to join handle.
    join_handles: Arc<Mutex<HashMap<usize, JoinHandle<()>>>>,
    /// Sender for the signal to shutdown.
    shutdown_sender: watch::Sender<()>,
    /// Receiver for the signal to shutdown.
    shutdown_receiver: ShutdownReceiver,
}

impl TaskManager {
    /// Creates a new task manager.
    pub(super) fn new() -> Self {
        let (shutdown_sender, shutdown_watch_receiver) = watch::channel(());

        Self {
            task_counter: 0,
            join_handles: Default::default(),
            shutdown_sender,
            shutdown_receiver: ShutdownReceiver(shutdown_watch_receiver),
        }
    }

    /// Spawns a new tasks.
    ///
    /// Tasks are expected to honor received shutdown signals from the manager, that is they need to
    /// shutdown once they receive a value on the `Receiver`. No return values are supported for
    /// spawned tasks.
    ///
    /// If the internal log has been poisoned, outputs a warning, but otherwise keeps on going.
    pub(super) fn spawn<F, G>(&mut self, gen: F)
    where
        F: FnOnce(ShutdownReceiver) -> G,
        G: Future<Output = ()> + Send + 'static,
    {
        let task_id = self.task_counter;
        self.task_counter += 1;

        let join_handles = self.join_handles.clone();

        let fut = gen(self.shutdown_receiver.clone());
        let join_handle = tokio::spawn(async move {
            fut.await;

            // To avoid creating an infinite amount of join handles, we remove our entry from the
            // join handles map, if still in there. If the lock is poisoned we skip this step.
            if let Ok(mut join_handles) = join_handles.lock() {
                join_handles.remove(&task_id);
            }
        });

        // Now that the task has been spawned, insert into join handle list.
        if let Ok(mut join_handles) = self.join_handles.lock() {
            join_handles.insert(task_id, join_handle);
        } else {
            // This is problematic, but no reason to crash in some cases.
            warn!("join handles lock has been poisoned, no freeing entry");
        }
    }

    /// Waits for all background tasks to shut down.
    ///
    /// Returns the number of tasks that were not joined successfully in time.
    ///
    /// If the lock has been poisoned, `Err(())` is returned, as it is impossible to properly join
    /// the tasks at that point. They will still be asked to shutdown.
    async fn wait_for_shutdown(self, timeout: Duration) -> Result<usize, ()> {
        // Dropping the sender will cause all receivers to shutdown.
        drop(self.shutdown_sender);

        let mut join_handles = self.join_handles.lock().map_err(|_| ())?;

        // We create a stream of join results that we can collect in parallel.
        let mut joins: FuturesUnordered<_> = join_handles
            .drain()
            .map(|(task_id, join_handle)| async move {
                (
                    task_id,
                    match time::timeout(timeout, join_handle).await {
                        Ok(Ok(())) => ShutdownOutcome::Ok,
                        Ok(Err(join_err)) => ShutdownOutcome::JoinFailure(join_err),
                        Err(_) => ShutdownOutcome::TimedOut,
                    },
                )
            })
            .collect();

        // Collect all the join handles and output appropriate errors.
        let mut failed = 0;
        while let Some((task_id, outcome)) = joins.next().await {
            match outcome {
                ShutdownOutcome::Ok => {
                    trace!(task_id, "successfully joined task");
                }
                ShutdownOutcome::JoinFailure(err) => {
                    warn!(task_id, %err, "could not join task");
                    failed += 1;
                }
                ShutdownOutcome::TimedOut => {
                    error!(task_id, "task joining timed out and handle lost");
                    failed += 1;
                }
            }
        }

        Ok(failed)
    }
}
