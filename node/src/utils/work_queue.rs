//! Work queue for finite work.
//!
//! A queue that allows for processing a variable amount of work that may spawn more jobs, but is
//! expected to finish eventually.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use futures::{stream, Stream};
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
/// #![allow(non_snake_case)]
/// # use std::{sync::Arc, time::Duration};
/// #
/// # use futures::stream::{futures_unordered::FuturesUnordered, StreamExt};
/// #
/// # use casper_node::utils::work_queue::WorkQueue;
/// #
/// type DemoJob = (&'static str, usize);
///
/// /// Job processing function.
/// ///
/// /// For a given job `(name, n)`, returns two jobs with `n = n - 1`, unless `n == 0`.
/// async fn process_job(job: DemoJob) -> Vec<DemoJob> {
///     tokio::time::sleep(Duration::from_millis(25)).await;
///
///     let (tag, n) = job;
///
///     if n == 0 {
///         Vec::new()
///     } else {
///         vec![(tag, n - 1), (tag, n - 1)]
///     }
/// }
///
/// /// Job-processing worker.
/// ///
/// /// `id` is the worker ID for logging.
/// async fn worker(id: usize, q: Arc<WorkQueue<DemoJob>>) {
///     println!("worker {}: init", id);
///
///     while let Some(job) = q.next_job().await {
///         println!("worker {}: start job {:?}", id, job.inner());
///         for new_job in process_job(job.inner().clone()).await {
///             q.push_job(new_job);
///         }
///         println!("worker {}: finish job {:?}", id, job.inner());
///     }
///
///     println!("worker {}: shutting down", id);
/// }
///
/// const WORKER_COUNT: usize = 3;
/// #
/// # async fn test_func() {
/// let q = Arc::new(WorkQueue::default());
/// q.push_job(("A", 3));
///
/// let workers: FuturesUnordered<_> = (0..WORKER_COUNT).map(|id| worker(id, q.clone())).collect();
///
/// // Wait for all workers to finish.
/// workers.for_each(|_| async move {}).await;
/// # }
/// # let rt = tokio::runtime::Runtime::new().unwrap();
/// # let handle = rt.handle();
/// # handle.block_on(test_func());
/// ```
#[derive(Debug)]
pub struct WorkQueue<T> {
    /// Inner workings of the queue.
    inner: Mutex<QueueInner<T>>,
    /// Notifier for waiting tasks.
    notify: Notify,
}

/// Queue inner state.
#[derive(Debug)]
struct QueueInner<T> {
    /// Jobs currently in the queue.
    jobs: VecDeque<T>,
    /// Number of jobs that have been popped from the queue using `next_job` but not finished.
    in_progress: usize,
}

// Manual default implementation, since the derivation would require a `T: Default` trait bound.
impl<T> Default for WorkQueue<T> {
    fn default() -> Self {
        Self {
            inner: Default::default(),
            notify: Default::default(),
        }
    }
}

impl<T> Default for QueueInner<T> {
    fn default() -> Self {
        Self {
            jobs: Default::default(),
            in_progress: Default::default(),
        }
    }
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
                let mut inner = self.inner.lock().expect("lock poisoned");
                match inner.jobs.pop_front() {
                    Some(job) => {
                        // We got a job, increase the `in_progress` count and return.
                        inner.in_progress += 1;
                        return Some(JobHandle {
                            job,
                            queue: self.clone(),
                        });
                    }
                    None => {
                        // No job found. Check if we are completely done.
                        if inner.in_progress == 0 {
                            // No more jobs, no jobs in progress. We are done!
                            return None;
                        }

                        // Otherwise, we have to wait.
                        waiting = self.notify.notified();
                    }
                }
            }

            // Note: Any notification sent while executing this segment (after the guard has been
            // dropped, but before `waiting.await` has been entered) will still be picked up by
            // `waiting.await`, as the call to `notified()` marks the beginning of the waiting
            // period, not `waiting.await`. See `tests::notification_assumption_holds`.

            // After freeing the lock, wait for a new job to arrive or be finished.
            waiting.await;
        }
    }

    /// Pushes a job onto the queue.
    ///
    /// If there are any worker waiting on `next_job`, one of them will receive the job.
    pub fn push_job(&self, job: T) {
        let mut inner = self.inner.lock().expect("lock poisoned");

        inner.jobs.push_back(job);
        self.notify.notify_waiters();
    }

    /// Creates a streaming consumer of the work queue.
    #[inline]
    pub fn to_stream(self: Arc<Self>) -> impl Stream<Item = JobHandle<T>> {
        stream::unfold(self, |work_queue| async move {
            let next = work_queue.next_job().await;
            next.map(|handle| (handle, work_queue))
        })
    }

    /// Mark job completion.
    ///
    /// This is an internal function to be used by `JobHandle`, which locks the internal queue and
    /// decreases the in-progress count by one.
    fn complete_job(&self) {
        let mut inner = self.inner.lock().expect("lock poisoned");

        inner.in_progress -= 1;
        self.notify.notify_waiters();
    }
}

/// Handle containing a job.
///
/// Holds a job popped from the job queue.
///
/// The job will be considered completed once `JobHandle` has been dropped.
#[derive(Debug)]
pub struct JobHandle<T> {
    /// The protected job.
    job: T,
    /// Queue job was removed from.
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

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            atomic::{AtomicU32, Ordering},
            Arc,
        },
        time::Duration,
    };

    use futures::{FutureExt, StreamExt};
    use tokio::sync::Notify;

    use super::WorkQueue;

    #[derive(Debug)]
    struct TestJob(u32);

    // Verify that the assumption made about `Notification` -- namely that a call to `notified()` is
    // enough to "register" the waiter -- holds.
    #[test]
    fn notification_assumption_holds() {
        let not = Notify::new();

        // First attempt to await a notification, should return pending.
        assert!(not.notified().now_or_never().is_none());

        // Second, we notify, then try notification again. Should also return pending, as we were
        // "not around" when the notification happened.
        not.notify_waiters();
        assert!(not.notified().now_or_never().is_none());

        // Finally, we "register" for notification beforehand.
        let waiter = not.notified();
        not.notify_waiters();
        assert!(waiter.now_or_never().is_some());
    }

    /// Process a job, sleeping a short amout of time on every 5th job.
    async fn job_worker_simple(queue: Arc<WorkQueue<TestJob>>, sum: Arc<AtomicU32>) {
        while let Some(job) = queue.next_job().await {
            if job.inner().0 % 5 == 0 {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }

            sum.fetch_add(job.inner().0, Ordering::SeqCst);
        }
    }

    /// Process a job, sleeping a short amount of time on every job.
    ///
    /// Spawns two additional jobs for every job processed, decreasing the job number until reaching
    /// zero.
    async fn job_worker_binary(queue: Arc<WorkQueue<TestJob>>, sum: Arc<AtomicU32>) {
        while let Some(job) = queue.next_job().await {
            tokio::time::sleep(Duration::from_millis(10)).await;

            sum.fetch_add(job.inner().0, Ordering::SeqCst);

            if job.inner().0 > 0 {
                queue.push_job(TestJob(job.inner().0 - 1));
                queue.push_job(TestJob(job.inner().0 - 1));
            }
        }
    }

    #[tokio::test]
    async fn empty_queue_exits_immediately() {
        let q: Arc<WorkQueue<TestJob>> = Arc::new(Default::default());
        assert!(q.next_job().await.is_none());
    }

    #[tokio::test]
    async fn large_front_loaded_queue_terminates() {
        let num_jobs = 1_000;
        let q: Arc<WorkQueue<TestJob>> = Arc::new(Default::default());
        for job in (0..num_jobs).map(TestJob) {
            q.push_job(job);
        }

        let mut workers = Vec::new();
        let output = Arc::new(AtomicU32::new(0));
        for _ in 0..3 {
            workers.push(tokio::spawn(job_worker_simple(q.clone(), output.clone())));
        }

        // We use a different pattern for waiting here, see the doctest for a solution that does not
        // spawn.
        for worker in workers {
            worker.await.expect("task panicked");
        }

        let expected_total = (num_jobs * (num_jobs - 1)) / 2;
        assert_eq!(output.load(Ordering::SeqCst), expected_total);
    }

    #[tokio::test]
    async fn stream_interface_works() {
        let num_jobs = 1_000;
        let q: Arc<WorkQueue<TestJob>> = Arc::new(Default::default());
        for job in (0..num_jobs).map(TestJob) {
            q.push_job(job);
        }

        let mut current = 0;
        let mut stream = Box::pin(q.to_stream());
        while let Some(job) = stream.next().await {
            assert_eq!(job.inner().0, current);
            current += 1;
        }
    }

    #[tokio::test]
    async fn complex_queue_terminates() {
        let num_jobs = 5;
        let q: Arc<WorkQueue<TestJob>> = Arc::new(Default::default());
        for _ in 0..num_jobs {
            q.push_job(TestJob(num_jobs));
        }

        let mut workers = Vec::new();
        let output = Arc::new(AtomicU32::new(0));
        for _ in 0..3 {
            workers.push(tokio::spawn(job_worker_binary(q.clone(), output.clone())));
        }

        // We use a different pattern for waiting here, see the doctest for a solution that does not
        // spawn.
        for worker in workers {
            worker.await.expect("task panicked");
        }

        // A single job starting at `k` will add `SUM_{n=0}^{k} (k-n) * 2^n`, which is
        // 57 for `k=5`. We start 5 jobs, so we expect `5 * 57 = 285` to be the result.
        let expected_total = 285;
        assert_eq!(output.load(Ordering::SeqCst), expected_total);
    }
}
