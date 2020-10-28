use std::{
    collections::BTreeMap,
    ops::{Add, AddAssign, Div},
};

pub(crate) fn weighted_median<T, I, Weight>(iterator: I) -> Option<T>
where
    T: Ord,
    Weight: Default
        + Add<Weight, Output = Weight>
        + AddAssign<Weight>
        + PartialOrd
        + Copy
        + Div<u64, Output = Weight>,
    I: IntoIterator<Item = (T, Weight)>,
{
    let (value_weights, total_weight): (_, Weight) = iterator.into_iter().fold(
        (BTreeMap::<T, Weight>::new(), Default::default()),
        |(mut weights, total), (value, weight)| {
            *weights.entry(value).or_default() += weight;
            (weights, total + weight)
        },
    );
    let mut counted: Weight = Default::default();
    for (value, weight) in value_weights {
        counted += weight;
        if counted > total_weight / 2u64 {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod misc_tests {
    use std::iter;

    use super::weighted_median;

    fn median<T, I>(iterator: I) -> Option<T>
    where
        T: Ord,
        I: IntoIterator<Item = T>,
    {
        weighted_median(iterator.into_iter().zip(iter::repeat(1u64)))
    }

    #[test]
    fn test_median() {
        assert_eq!(None, median::<u8, _>(vec![]));
        assert_eq!(Some(2), median(vec![2u8, 3, 1]));
        assert_eq!(Some(3), median(vec![2u8, 3, 4, 1]));
        assert_eq!(Some(3), median(vec![1u8, 3, 4, 1]));
        assert_eq!(Some(3), median(vec![2u8, 3, 4, 1, 5]));
        assert_eq!(Some(2), median(vec![2u8, 3, 4, 1, 2]));
        assert_eq!(Some(3), median(vec![2u8, 3, 4, 1, 3]));
    }

    #[test]
    fn test_weighted_median() {
        assert_eq!(Some(1), weighted_median(vec![(1u8, 3), (2u8, 1), (3u8, 1)]));
        assert_eq!(Some(2), weighted_median(vec![(1u8, 2), (2u8, 1), (3u8, 1)]));
        assert_eq!(Some(2), weighted_median(vec![(1u8, 2), (2u8, 2), (3u8, 1)]));
        assert_eq!(Some(3), weighted_median(vec![(1u8, 2), (2u8, 2), (3u8, 8)]));
    }
}
