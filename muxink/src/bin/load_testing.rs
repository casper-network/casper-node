use std::time::{Duration, Instant};

use futures::{FutureExt, SinkExt, StreamExt};
use rand::{distributions::Standard, thread_rng, Rng};

use muxink::{self, testing::fixtures::TwoWayFixtures};

macro_rules! p {
    ($start:expr, $($arg:tt)*) => {{
        let time = $start.elapsed().as_millis();
        print!("{time} - ");
        println!($($arg)*);
    }};
}

// This binary is useful for probing memory consumption of muxink.
// Probably you want `heaptrack` installed to run this. https://github.com/KDE/heaptrack
//
// Test with:
// ```
// cargo build --profile release-with-debug --bin load_testing --features testing && \
// heaptrack -o ~/heap ../target/release-with-debug/load_testing
// ```

fn main() {
    let s = Instant::now();
    p!(s, "started load_testing binary");

    let message_size = 1024 * 1024 * 8;
    let rand_bytes: Vec<u8> = thread_rng()
        .sample_iter(Standard)
        .take(message_size)
        .collect();

    futures::executor::block_on(async move {
        test_ever_larger_buffers_matching_window_size(&s, rand_bytes.clone()).await;
        test_cycling_full_buffer(&s, rand_bytes.clone(), 1, 1000).await;
        test_cycling_full_buffer(&s, rand_bytes.clone(), 10, 100).await;
        test_cycling_full_buffer(&s, rand_bytes.clone(), 100, 10).await;
    });
    p!(s, "load_testing binary finished");
}

async fn test_ever_larger_buffers_matching_window_size(s: &Instant, rand_bytes: Vec<u8>) {
    p!(s, "testing buffers (filled to window size)");
    for buffer_size in 1..100 {
        let window_size = buffer_size as u64;
        p!(
            s,
            "buffer size = {buffer_size}, expected mem consumption ~= {}",
            rand_bytes.len() * buffer_size
        );
        let TwoWayFixtures {
            mut client,
            mut server,
        } = TwoWayFixtures::new_with_window(buffer_size, window_size);
        for _message_sequence in 0..buffer_size {
            client.send(rand_bytes.clone().into()).await.unwrap();
        }
        for _message_sequence in 0..buffer_size {
            server.next().now_or_never().unwrap();
        }
    }
}

async fn test_cycling_full_buffer(
    s: &Instant,
    rand_bytes: Vec<u8>,
    buffer_size: usize,
    cycles: u32,
) {
    p!(
        s,
        "testing cycling buffers (fill to window size, then empty)"
    );
    let window_size = buffer_size as u64;
    p!(
        s,
        "buffer size = {buffer_size}, expected mem consumption ~= {}",
        rand_bytes.len() * buffer_size
    );
    let TwoWayFixtures {
        mut client,
        mut server,
    } = TwoWayFixtures::new_with_window(buffer_size, window_size);
    for cycles in 0..cycles {
        for _message_sequence in 0..buffer_size {
            client.send(rand_bytes.clone().into()).await.unwrap();
        }
        for _message_sequence in 0..buffer_size {
            server.next().now_or_never().unwrap();
        }
    }
}
