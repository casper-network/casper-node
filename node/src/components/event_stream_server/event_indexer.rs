use std::{fs, path::PathBuf};

use datasize::DataSize;
use tracing::{debug, warn};

const CACHE_FILENAME: &str = "sse_index";

pub(super) type EventIndex = u32;

#[derive(Debug, DataSize)]
pub(super) struct EventIndexer {
    index: EventIndex,
    persistent_cache: PathBuf,
}

impl EventIndexer {
    pub(super) fn new(storage_path: PathBuf) -> Self {
        let persistent_cache = storage_path.join(CACHE_FILENAME);
        let mut bytes = EventIndex::default().to_le_bytes();
        match fs::read(&persistent_cache) {
            Err(error) => {
                if persistent_cache.exists() {
                    warn!(
                        file = %persistent_cache.display(),
                        %error,
                        "failed to read sse cache file"
                    );
                }
            }
            Ok(cached_bytes) => {
                if cached_bytes.len() == bytes.len() {
                    bytes.copy_from_slice(cached_bytes.as_slice());
                } else {
                    warn!(
                        file = %persistent_cache.display(),
                        byte_count = %cached_bytes.len(),
                        "failed to parse sse cache file"
                    );
                }
            }
        }

        let index = EventIndex::from_le_bytes(bytes);
        debug!(%index, "initialized sse index");

        EventIndexer {
            index,
            persistent_cache,
        }
    }

    pub(super) fn next_index(&mut self) -> EventIndex {
        let index = self.index;
        self.index = index.wrapping_add(1);
        index
    }
}

impl Drop for EventIndexer {
    fn drop(&mut self) {
        if let Err(error) = fs::write(&self.persistent_cache, self.index.to_le_bytes()) {
            warn!(
                file = %self.persistent_cache.display(),
                %error,
                "failed to write sse cache file"
            );
        }
        debug!(
            file = %self.persistent_cache.display(),
            index = %self.index,
            "cached sse index to file"
        );
    }
}

#[cfg(test)]
mod tests {
    use std::iter;

    use super::*;
    use crate::logging;

    #[test]
    fn should_persist_in_cache() {
        let _ = logging::init();
        let tempdir = tempfile::tempdir().unwrap();

        // This represents a single session where five events are produced before the session ends.
        let init_and_increment_by_five = |expected_first_index: EventIndex| {
            let mut event_indexer = EventIndexer::new(tempdir.path().to_path_buf());
            for i in 0..5 {
                assert_eq!(event_indexer.next_index(), expected_first_index + i);
            }
            // Explicitly drop, just to be clear that the cache write is being triggered.
            drop(event_indexer);
        };

        // Should start at 0 when no cache file exists.
        init_and_increment_by_five(0);

        // Should keep reading and writing to cache over ten subsequent sessions.
        for session in 1..11 {
            init_and_increment_by_five(session * 5);
        }
    }

    #[test]
    fn should_wrap() {
        let _ = logging::init();
        let tempdir = tempfile::tempdir().unwrap();

        let mut event_indexer = EventIndexer::new(tempdir.path().to_path_buf());
        event_indexer.index = EventIndex::MAX;

        assert_eq!(event_indexer.next_index(), EventIndex::MAX);
        assert_eq!(event_indexer.next_index(), 0);
    }

    #[test]
    fn should_reset_index_on_cache_read_failure() {
        let _ = logging::init();
        let tempdir = tempfile::tempdir().unwrap();

        // Create a folder with the same name as the cache file to cause reading to fail.
        fs::create_dir(tempdir.path().join(CACHE_FILENAME)).unwrap();
        let mut event_indexer = EventIndexer::new(tempdir.path().to_path_buf());
        assert_eq!(event_indexer.next_index(), 0);
    }

    #[test]
    fn should_reset_index_on_corrupt_cache() {
        let _ = logging::init();
        let tempdir = tempfile::tempdir().unwrap();

        {
            // Create the cache file with too few bytes to be parsed as an `Index`.
            let index: EventIndex = 1;
            fs::write(
                tempdir.path().join(CACHE_FILENAME),
                &index.to_le_bytes()[1..],
            )
            .unwrap();

            let mut event_indexer = EventIndexer::new(tempdir.path().to_path_buf());
            assert_eq!(event_indexer.next_index(), 0);
        }

        {
            // Create the cache file with too many bytes to be parsed as an `Index`.
            let index: EventIndex = 1;
            let bytes: Vec<u8> = index
                .to_le_bytes()
                .iter()
                .chain(iter::once(&0))
                .copied()
                .collect();
            fs::write(tempdir.path().join(CACHE_FILENAME), bytes).unwrap();

            let mut event_indexer = EventIndexer::new(tempdir.path().to_path_buf());
            assert_eq!(event_indexer.next_index(), 0);
        }
    }
}
