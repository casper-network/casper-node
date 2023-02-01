use std::fmt::{self, Display, Formatter};

use casper_types::bytesrepr::{self, FromBytes, ToBytes};
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
#[derive(Copy, Clone, Debug, Eq, PartialEq, DataSize, Ord, PartialOrd)]
pub(crate) struct Sequence {
    /// The upper bound (inclusive) of the sequence.
    high: u64,
    /// The lower bound (inclusive) of the sequence.
    low: u64,
}

impl Sequence {
    /// Constructs a new sequence using the bounds of `a` and `b`.
    ///
    /// `low` and `high` will be automatically determined.
    pub(super) fn new(a: u64, b: u64) -> Self {
        let (low, high) = if a <= b { (a, b) } else { (b, a) };
        Sequence { low, high }
    }

    /// Constructs a new sequence containing only `value`.
    fn single(value: u64) -> Self {
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

    /// Returns the inclusive high end of the sequence.
    pub(crate) fn high(&self) -> u64 {
        self.high
    }

    /// Returns the inclusive low end of the sequence.
    pub(crate) fn low(&self) -> u64 {
        self.low
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
#[cfg_attr(test, derive(Clone))]
pub(super) struct DisjointSequences {
    sequences: Vec<Sequence>,
}

impl DisjointSequences {
    /// Constructs disjoint sequences from one initial sequence.
    ///
    /// Note: Use [`Default::default()`] to create an empty set of sequences.
    pub(super) fn new(initial_sequence: Sequence) -> Self {
        DisjointSequences {
            sequences: vec![initial_sequence],
        }
    }

    /// Inserts `value` into the appropriate sequence and merges sequences if required.
    ///
    /// Returns `true` if `value` was not previously contained in the disjoint sequences.
    ///
    /// Note, this method is efficient where `value` is one greater than the current highest value.
    /// However, it's not advisable to use this method in a loop to rebuild a `DisjointSequences`
    /// from a large collection of randomly-ordered values.  In that case, it is very much more
    /// efficient to use `DisjointSequences::from(mut input: Vec<u64>)`.
    pub(super) fn insert(&mut self, value: u64) -> bool {
        let mut iter_mut = self.sequences.iter_mut().enumerate().peekable();

        // The index at which to add a new `Sequence` containing only `value`.
        let mut maybe_insertion_index = Some(0);
        // The index of a `Sequence` to be removed due to the insertion of `value` causing two
        // consecutive sequences to become contiguous.
        let mut maybe_removal_index = None;
        let mut added_new_value = true;
        while let Some((index, sequence)) = iter_mut.next() {
            match sequence.try_insert(value) {
                InsertOutcome::ExtendedHigh => {
                    // We should exit the loop, and we don't need to add a new sequence; we only
                    // need to check for merges of sequences when we get `ExtendedLow` since we're
                    // iterating the sequences from high to low.
                    maybe_insertion_index = None;
                    break;
                }
                InsertOutcome::AlreadyInSequence => {
                    // We should exit the loop, and we don't need to add a new sequence.
                    maybe_insertion_index = None;
                    added_new_value = false;
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
            self.sequences
                .insert(index_to_insert, Sequence::single(value));
        }

        if let Some(index_to_remove) = maybe_removal_index {
            let _ = self.sequences.remove(index_to_remove);
        }

        trace!(%self, "current state of disjoint sequences");
        added_new_value
    }

    /// Returns the highest sequence, or `None` if there are no sequences.
    pub(super) fn highest_sequence(&self) -> Option<&Sequence> {
        self.sequences.first()
    }

    /// Returns all the sequences, if any.
    pub(super) fn sequences(&self) -> &Vec<Sequence> {
        &self.sequences
    }

    /// Reduces the sequence(s), keeping all entries below and including `max_value`.  If
    /// `max_value` is not already included in a sequence, it will not be added.
    ///
    /// If the current highest value is lower than `max_value`, or if there are no sequences, this
    /// has no effect.
    pub(super) fn truncate(&mut self, max_value: u64) {
        self.sequences.retain_mut(|sequence| {
            if sequence.high <= max_value {
                // Keep this sequence unchanged.
                return true;
            }

            if sequence.low > max_value {
                // Delete this entire sequence.
                return false;
            }

            // This sequence contains `max_value`, so keep the sequence, but reduce its high value.
            sequence.high = max_value;
            true
        })
    }
}
#[cfg(test)]
impl DisjointSequences {
    /// Inserts multiple values produced by the given iterator.
    fn extend<T: IntoIterator<Item = u64>>(&mut self, iter: T) {
        iter.into_iter().for_each(|height| {
            self.insert(height);
        })
    }

    /// Returns `true` if `value` exists in the disjoint sequences.
    fn contains(&self, value: u64) -> bool {
        self.sequences
            .iter()
            .any(|sequence| value >= sequence.low && value <= sequence.high)
    }
}

impl FromBytes for Sequence {
    #[inline]
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (high, bytes) = u64::from_bytes(bytes)?;
        let (low, bytes) = u64::from_bytes(bytes)?;

        Ok((Sequence { high, low }, bytes))
    }
}

impl ToBytes for Sequence {
    #[inline]
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buf = Vec::new();
        self.write_bytes(&mut buf)?;
        Ok(buf)
    }

    #[inline]
    fn serialized_length(&self) -> usize {
        self.high.serialized_length() + self.low.serialized_length()
    }

    #[inline]
    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.high.write_bytes(writer)?;
        self.low.write_bytes(writer)?;
        Ok(())
    }
}

impl FromBytes for DisjointSequences {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        Vec::<Sequence>::from_bytes(bytes)
            .map(|(sequences, remainder)| (DisjointSequences { sequences }, remainder))
    }

    #[inline]
    fn from_vec(bytes: Vec<u8>) -> Result<(Self, Vec<u8>), bytesrepr::Error> {
        Vec::<Sequence>::from_vec(bytes)
            .map(|(sequences, remainder)| (DisjointSequences { sequences }, remainder))
    }
}

impl ToBytes for DisjointSequences {
    #[inline]
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        self.sequences.to_bytes()
    }

    #[inline]
    fn serialized_length(&self) -> usize {
        self.sequences.serialized_length()
    }

    fn into_bytes(self) -> Result<Vec<u8>, bytesrepr::Error>
    where
        Self: Sized,
    {
        self.sequences.into_bytes()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.sequences.write_bytes(writer)
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
                    let mut sequence = Sequence::single(low);
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

    fn new_sequence(a: u64, b: u64) -> Sequence {
        let (low, high) = if a <= b { (a, b) } else { (b, a) };
        assert!(low <= high);
        Sequence { low, high }
    }

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
            let insertion_result = !disjoint_sequences.contains(value);
            assert_eq!(insertion_result, disjoint_sequences.insert(value));
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
            assert!(disjoint_sequences.insert(value));
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
    fn should_get_highest_sequence() {
        let mut disjoint_sequences = DisjointSequences::default();
        assert_eq!(disjoint_sequences.highest_sequence(), None);

        disjoint_sequences.extend([1]);
        assert_eq!(
            disjoint_sequences.highest_sequence(),
            Some(&Sequence { low: 1, high: 1 })
        );

        disjoint_sequences.extend([5, 6]);
        assert_eq!(
            disjoint_sequences.highest_sequence(),
            Some(&Sequence { low: 5, high: 6 })
        );

        disjoint_sequences.extend([8, 9]);
        assert_eq!(
            disjoint_sequences.highest_sequence(),
            Some(&Sequence { low: 8, high: 9 })
        );
    }

    #[test]
    fn should_truncate() {
        const SEQ_HIGH: Sequence = Sequence { high: 11, low: 9 };
        const SEQ_MID: Sequence = Sequence { high: 6, low: 6 };
        const SEQ_LOW: Sequence = Sequence { high: 3, low: 1 };
        let initial_sequences = DisjointSequences {
            sequences: vec![SEQ_HIGH, SEQ_MID, SEQ_LOW],
        };

        // Truncate with `max_value` greater or equal to current highest value should be a no-op.
        let mut disjoint_sequences = initial_sequences.clone();
        disjoint_sequences.truncate(12);
        assert_eq!(disjoint_sequences.sequences, initial_sequences.sequences);
        disjoint_sequences.truncate(11);
        assert_eq!(disjoint_sequences.sequences, initial_sequences.sequences);

        // Truncate with `max_value` between two sequences should cause the higher sequences to get
        // removed and the lower ones retained unchanged.
        disjoint_sequences = initial_sequences.clone();
        disjoint_sequences.truncate(SEQ_HIGH.low - 1);
        assert_eq!(disjoint_sequences.sequences, vec![SEQ_MID, SEQ_LOW]);

        disjoint_sequences = initial_sequences.clone();
        disjoint_sequences.truncate(SEQ_MID.high);
        assert_eq!(disjoint_sequences.sequences, vec![SEQ_MID, SEQ_LOW]);

        disjoint_sequences = initial_sequences.clone();
        disjoint_sequences.truncate(SEQ_MID.low - 1);
        assert_eq!(disjoint_sequences.sequences, vec![SEQ_LOW]);

        disjoint_sequences = initial_sequences.clone();
        disjoint_sequences.truncate(SEQ_LOW.high);
        assert_eq!(disjoint_sequences.sequences, vec![SEQ_LOW]);

        // Truncate with `max_value` lower than the lowest value should cause all sequences to get
        // removed.
        disjoint_sequences = initial_sequences.clone();
        disjoint_sequences.truncate(SEQ_LOW.low - 1);
        assert!(disjoint_sequences.sequences.is_empty());

        // Truncate with `max_value` within a sequence should cause that sequence to get updated,
        // any higher sequences to get removed, and any lower ones retained unchanged.
        disjoint_sequences = initial_sequences.clone();
        let max_value = SEQ_HIGH.high - 1;
        disjoint_sequences.truncate(max_value);
        assert_eq!(
            disjoint_sequences.sequences,
            vec![new_sequence(max_value, SEQ_HIGH.low), SEQ_MID, SEQ_LOW]
        );

        disjoint_sequences = initial_sequences.clone();
        let max_value = SEQ_HIGH.low;
        disjoint_sequences.truncate(max_value);
        assert_eq!(
            disjoint_sequences.sequences,
            vec![new_sequence(max_value, SEQ_HIGH.low), SEQ_MID, SEQ_LOW]
        );

        disjoint_sequences = initial_sequences.clone();
        let max_value = SEQ_MID.low;
        disjoint_sequences.truncate(max_value);
        assert_eq!(disjoint_sequences.sequences, vec![SEQ_MID, SEQ_LOW]);

        disjoint_sequences = initial_sequences.clone();
        let max_value = SEQ_LOW.high - 1;
        disjoint_sequences.truncate(max_value);
        assert_eq!(
            disjoint_sequences.sequences,
            vec![new_sequence(max_value, SEQ_LOW.low)]
        );

        disjoint_sequences = initial_sequences;
        let max_value = SEQ_LOW.low;
        disjoint_sequences.truncate(max_value);
        assert_eq!(
            disjoint_sequences.sequences,
            vec![new_sequence(max_value, SEQ_LOW.low)]
        );

        // Truncate on an empty set of sequences should have no effect.
        disjoint_sequences = DisjointSequences::default();
        assert!(disjoint_sequences.sequences.is_empty());
        disjoint_sequences.truncate(100);
        assert!(disjoint_sequences.sequences.is_empty());
    }

    #[test]
    fn roundtrip_to_bytes() {
        let mut disjoint_sequences = DisjointSequences::default();

        disjoint_sequences.extend([4, 5, 6, 7, 8]);
        disjoint_sequences.extend([15, 16, 17, 18, 19, 20]);

        // should be represented logically as [(20 to 15), (8 to 4)] and serialize to a sequence of
        // `2u32 20u64 15u64 8u64 4u64`.

        let expected = [
            0x02, 0x00, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0F, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];
        let actual = disjoint_sequences.to_bytes().expect("serialization failed");
        assert_eq!(expected.as_slice(), &actual);

        let expected_inner_state = disjoint_sequences.sequences;
        let (restored, remainder) =
            DisjointSequences::from_bytes(&actual).expect("deserialization failed");
        assert!(remainder.is_empty());

        let (restored2, remainder) =
            DisjointSequences::from_vec(actual).expect("deserialization failed");
        assert!(remainder.is_empty());

        assert_eq!(restored.sequences, expected_inner_state);
        assert_eq!(restored2.sequences, expected_inner_state);
    }
}
