use std::fmt::{self, Display, Formatter};

use datasize::DataSize;
use itertools::Itertools;
use tracing::trace;

/// The outcome of an attempt to insert a value into a `Sequence`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum InsertOutcome {
    /// The value was greater than `Sequence::high + 1` and wasn't inserted.
    TooHigh,
    /// The value was inserted at the high end, and is now `Sequence::high`.
    ExtendedHigh,
    /// The value was a duplicate; inserted and didn't affect the high or low values.
    AlreadyInSequence,
    /// The value was inserted at the low end, and is now `Sequence::low`.
    ExtendedLow,
    /// The value was less than `Sequence::low - 1` and wasn't inserted.
    TooLow,
}

/// Represents a continuous sequence of `u64`s.
#[derive(Copy, Clone, Debug, Eq, PartialEq, DataSize)]
struct Sequence {
    /// The upper bound (inclusive) of the sequence.
    high: u64,
    /// The lower bound (inclusive) of the sequence.
    low: u64,
}

impl Sequence {
    /// Constructs a new sequence containing only `value`.
    fn new(value: u64) -> Self {
        Sequence {
            high: value,
            low: value,
        }
    }

    /// Tries to insert `value` into the sequence.
    ///
    /// Returns an outcome which indicates where the value was inserted if at all.
    fn try_insert(&mut self, value: u64) -> InsertOutcome {
        if value == self.high + 1 {
            self.high = value;
            InsertOutcome::ExtendedHigh
        } else if value >= self.low && value <= self.high {
            InsertOutcome::AlreadyInSequence
        } else if value + 1 == self.low {
            self.low = value;
            InsertOutcome::ExtendedLow
        } else if value > self.high {
            InsertOutcome::TooHigh
        } else {
            InsertOutcome::TooLow
        }
    }
}

/// Represents a collection of disjoint sequences of `u64`s.
///
/// The collection is kept ordered from high to low, and each entry represents a discrete portion of
/// the space from [0, u64::MAX] with a gap of at least 1 between each.
///
/// The collection is ordered this way to optimize insertion for the normal use case: adding
/// monotonically increasing values representing the latest block height.
///
/// As values are inserted, if two separate sequences become contiguous, they are merged into a
/// single sequence.
///
/// For example, if `sequences` contains `[9,9], [7,3]` and `8` is inserted, then `sequences` will
/// be reduced to `[9,3]`.
#[derive(Default, Debug, DataSize)]
pub(super) struct DisjointSequences {
    sequences: Vec<Sequence>,
}

impl DisjointSequences {
    /// Inserts `value` into the appropriate sequence and merges sequences if required.
    ///
    /// Note, this method is efficient where `value` is one greater than the current highest value.
    /// However, it's not advisable to use this method in a loop to rebuild a `DisjointSequences`
    /// from a large collection of randomly-ordered values.  In that case, it is very much more
    /// efficient to use `DisjointSequences::from(mut input: Vec<u64>)`.
    pub(super) fn insert(&mut self, value: u64) {
        let mut iter_mut = self.sequences.iter_mut().enumerate().peekable();

        // The index at which to add a new `Sequence` containing only `value`.
        let mut maybe_insertion_index = Some(0);
        // The index of a `Sequence` to be removed due to the insertion of `value` causing two
        // consecutive sequences to become contiguous.
        let mut maybe_removal_index = None;
        while let Some((index, sequence)) = iter_mut.next() {
            match sequence.try_insert(value) {
                InsertOutcome::ExtendedHigh | InsertOutcome::AlreadyInSequence => {
                    // We should exit the loop, and we don't need to add a new sequence; we only
                    // need to check for merges of sequences when we get `ExtendedLow` since we're
                    // iterating the sequences from high to low.
                    maybe_insertion_index = None;
                    break;
                }
                InsertOutcome::TooHigh => {
                    // We should exit the loop and we need to add a new sequence at this index.
                    maybe_insertion_index = Some(index);
                    break;
                }
                InsertOutcome::TooLow => {
                    // We need to add a new sequence immediately after this one if this is the last
                    // sequence.  Continue iterating in case this is not the last sequence.
                    maybe_insertion_index = Some(index + 1);
                }
                InsertOutcome::ExtendedLow => {
                    // We should exit the loop, and we don't need to add a new sequence.
                    maybe_insertion_index = None;
                    // If the next sequence is now contiguous with this one, update this one's low
                    // value and set the next sequence to be removed.
                    if let Some((next_index, next_sequence)) = iter_mut.peek() {
                        if next_sequence.high + 1 == sequence.low {
                            sequence.low = next_sequence.low;
                            maybe_removal_index = Some(*next_index);
                        }
                    }
                    break;
                }
            };
        }

        if let Some(index_to_insert) = maybe_insertion_index {
            let _ = self.sequences.insert(index_to_insert, Sequence::new(value));
        }

        if let Some(index_to_remove) = maybe_removal_index {
            let _ = self.sequences.remove(index_to_remove);
        }

        trace!(%self, "current state of disjoint sequences");
    }

    /// Inserts multiple values produced by the given interator.
    pub(super) fn extend<T>(&mut self, iter: T)
    where
        T: IntoIterator<Item = u64>,
    {
        iter.into_iter().for_each(|height| self.insert(height))
    }

    /// Returns the `low` value from the highest sequence, or `u64::MAX` if there are no sequences.
    pub(super) fn highest_sequence_low_value(&self) -> u64 {
        self.sequences
            .first()
            .map(|sequence| sequence.low)
            .unwrap_or(u64::MAX)
    }
}

/// This impl is provided to allow for efficient re-building of a `DisjointSequences` from a large,
/// randomly-ordered set of values.
impl From<Vec<u64>> for DisjointSequences {
    fn from(mut input: Vec<u64>) -> Self {
        input.sort_unstable();

        let sequences = input
            .drain(..)
            .peekable()
            .batching(|iter| match iter.next() {
                None => None,
                Some(low) => {
                    let mut sequence = Sequence::new(low);
                    while let Some(i) = iter.peek() {
                        if *i == sequence.high + 1 {
                            sequence.high = iter.next().unwrap();
                        }
                    }
                    Some(sequence)
                }
            })
            .collect();

        DisjointSequences { sequences }
    }
}

impl Display for DisjointSequences {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let mut iter = self.sequences.iter().peekable();
        while let Some(sequence) = iter.next() {
            write!(formatter, "[{}, {}]", sequence.high, sequence.low)?;
            if iter.peek().is_some() {
                write!(formatter, ", ")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use rand::{seq::SliceRandom, Rng};

    use super::*;

    fn assert_matches(actual: &DisjointSequences, expected: &BTreeSet<u64>) {
        let mut actual_set = BTreeSet::new();
        for sequence in &actual.sequences {
            for i in sequence.low..=sequence.high {
                assert!(actual_set.insert(i));
            }
        }
        assert_eq!(&actual_set, expected)
    }

    #[test]
    fn should_insert_all_u8s_including_duplicates() {
        let mut rng = crate::new_rng();

        let mut disjoint_sequences = DisjointSequences::default();
        let mut expected = BTreeSet::new();

        while disjoint_sequences.sequences != vec![Sequence { high: 255, low: 0 }] {
            let value = rng.gen::<u8>() as u64;
            disjoint_sequences.insert(value);
            expected.insert(value);
            assert_matches(&disjoint_sequences, &expected);
        }
    }

    #[test]
    fn should_extend() {
        let to_be_inserted = vec![5_u64, 4, 3, 2, 1];
        let mut expected = BTreeSet::new();
        expected.extend(to_be_inserted.clone());

        let mut disjoint_sequences = DisjointSequences::default();
        disjoint_sequences.extend(to_be_inserted);
        assert_matches(&disjoint_sequences, &expected);

        // Extending with empty set should not modify the sequences.
        disjoint_sequences.extend(Vec::<u64>::new());
        assert_matches(&disjoint_sequences, &expected);
    }

    #[test]
    fn should_insert_with_no_duplicates() {
        const MAX: u64 = 1000;

        let mut rng = crate::new_rng();

        let mut values = (0..=MAX).collect::<Vec<u64>>();
        values.shuffle(&mut rng);

        let mut disjoint_sequences = DisjointSequences::default();
        let mut expected = BTreeSet::new();

        for value in values {
            disjoint_sequences.insert(value);
            expected.insert(value);
            assert_matches(&disjoint_sequences, &expected);
        }

        assert_eq!(
            disjoint_sequences.sequences,
            vec![Sequence { high: MAX, low: 0 }]
        );
    }

    #[test]
    fn should_construct_from_random_set() {
        const MAX: u64 = 2_000_000;

        let mut rng = crate::new_rng();

        let mut values = (0..=MAX).collect::<Vec<u64>>();
        values.shuffle(&mut rng);

        let disjoint_sequences = DisjointSequences::from(values);
        assert_eq!(
            disjoint_sequences.sequences,
            vec![Sequence { high: MAX, low: 0 }]
        );
    }

    #[test]
    fn should_get_highest_sequence_low_value() {
        let mut disjoint_sequences = DisjointSequences::default();
        assert_eq!(disjoint_sequences.highest_sequence_low_value(), u64::MAX);

        disjoint_sequences.insert(10);
        assert_eq!(disjoint_sequences.highest_sequence_low_value(), 10);

        disjoint_sequences.insert(11);
        assert_eq!(disjoint_sequences.highest_sequence_low_value(), 10);

        disjoint_sequences.insert(5);
        assert_eq!(disjoint_sequences.highest_sequence_low_value(), 10);

        disjoint_sequences.insert(6);
        assert_eq!(disjoint_sequences.highest_sequence_low_value(), 10);

        disjoint_sequences.insert(13);
        assert_eq!(disjoint_sequences.highest_sequence_low_value(), 13);

        disjoint_sequences.insert(14);
        assert_eq!(disjoint_sequences.highest_sequence_low_value(), 13);
    }
}
