//! Work queue demonstration.

use std::collections::VecDeque;

use futures::stream::{futures_unordered::FuturesUnordered, StreamExt};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::sync::Notify;

/// Multi-producer, multi-consumer async job queue with end conditions.
///
/// Keeps track of in-progress jobs and can indicate to workers that all work has been finished.
/// Intended to be used for jobs that will spawn other jobs during processing, but stop once all
/// jobs have finished.
///
/// # Example use
///
/// ```rust
/// // TODO: Convert `main` below to doctest.
/// ```
#[derive(Debug, Default)]
struct WorkQueue<T> {
    /// Jobs currently in the queue.
    jobs: Mutex<VecDeque<T>>,
    /// Number of jobs that have been popped from the queue using `next_job` but not finished.
    in_progress: Arc<AtomicUsize>,
    /// Notifier for waiting tasks.
    notify: Notify,
}

impl<T> WorkQueue<T> {
    /// Pop a job from the queue.
    ///
    /// If there is a job in the queue, returns the job and increases the internal in progress
    /// counter by one.
    ///
    /// If there are still jobs in progress, but none queued, waits until either of these conditions
    /// changes, then retries.
    ///
    /// If there are no jobs available and no jobs in progress, returns `None`.
    pub async fn next_job(self: &Arc<Self>) -> Option<JobHandle<T>> {
        loop {
            let waiting;
            {
                let mut jobs = self.jobs.lock().expect("lock poisoned");
                match jobs.pop_front() {
                    Some(job) => {
                        // We got a job, increase the `in_progress` count and return.
                        self.in_progress.fetch_add(1, Ordering::SeqCst);
                        return Some(JobHandle {
                            job,
                            queue: self.clone(),
                        });
                    }
                    None => {
                        // No job found. Check if we are completely done.
                        if self.in_progress.load(Ordering::SeqCst) == 0 {
                            // No more jobs, no jobs in progress. We are done!
                            return None;
                        }

                        // Otherwise, we have to wait.
                        waiting = self.notify.notified();
                    }
                }
            }

            // After freeing the lock, wait for a new job to arrive or be finished.
            waiting.await;
        }
    }

    /// Pushes a job onto the queue.
    ///
    /// If there are any worker waiting on `next_job`, one of them will receive the job.
    pub fn push_job(&self, job: T) {
        let mut guard = self.jobs.lock().expect("lock poisoned");

        guard.push_back(job);
        self.notify.notify_waiters();
    }

    /// Mark job completion.
    ///
    /// This is an internal function to be used by `JobHandle`, which locks the internal queue and
    /// decreases the in-progress count by one.
    fn complete_job(&self) {
        // We need to lock the queue to prevent someone adding a job while we are notifying workers
        // about the completion of what might appear to be the last job.
        let _guard = self.jobs.lock().expect("lock poisoned");

        self.in_progress.fetch_sub(1, Ordering::SeqCst);
        self.notify.notify_waiters();
    }
}

/// Handle containing a job.
///
/// Holds a job popped from the job queue.
///
/// The job will be considered completed once `JobHandle` has been dropped.
#[derive(Debug)]
struct JobHandle<T> {
    job: T,
    queue: Arc<WorkQueue<T>>,
}

impl<T> JobHandle<T> {
    /// Returns a reference to the inner job.
    pub fn inner(&self) -> &T {
        &self.job
    }
}

impl<T> Drop for JobHandle<T> {
    fn drop(&mut self) {
        self.queue.complete_job()
    }
}

// *** Demonstration ***

type DemoJob = (&'static str, usize);

async fn process_job(job: DemoJob) -> Vec<DemoJob> {
    tokio::time::sleep(Duration::from_millis(250)).await;

    let (tag, n) = job;

    if n == 0 {
        Vec::new()
    } else {
        vec![(tag, n - 1), (tag, n - 1)]
    }
}

async fn worker(id: usize, q: Arc<WorkQueue<DemoJob>>) {
    println!("worker {}: init", id);

    while let Some(job) = q.next_job().await {
        println!("worker {}: start job {:?}", id, job.inner());
        for new_job in process_job(job.inner().clone()).await {
            q.push_job(new_job);
        }
        println!("worker {}: finish job {:?}", id, job.inner());
    }

    println!("worker {}: shutting down", id);
}

const WORKER_COUNT: usize = 3;

#[tokio::main]
async fn main() {
    let q = Arc::new(WorkQueue::default());
    q.push_job(("A", 3));

    let workers: FuturesUnordered<_> = (0..WORKER_COUNT).map(|id| worker(id, q.clone())).collect();

    // Wait for all workers to finish.
    workers.for_each(|_| async move {}).await;
}
