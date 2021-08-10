use std::time::Duration;

use tokio::time;
use tracing::debug;

pub(crate) async fn advance_time(duration: Duration) {
    time::pause();
    time::advance(duration).await;
    time::resume();
    debug!("advanced time by {} secs", duration.as_secs());
}
