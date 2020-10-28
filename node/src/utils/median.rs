use std::{
    collections::BTreeMap,
    ops::{Add, AddAssign},
};

pub(crate) fn weighted_median<T, I, Weight>(iterator: I) -> Option<T>
where
    T: Ord,
    Weight: Default + Add<Weight, Output = Weight> + AddAssign<Weight> + PartialOrd + Copy,
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
    let mut result = None;
    for (value, weight) in value_weights {
        result = Some(value);
        counted += weight;
        if counted + counted >= total_weight {
            break;
        }
    }
    result
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
        weighted_median(iterator.into_iter().zip(iter::repeat(1usize)))
    }

    #[test]
    fn test_median() {
        assert_eq!(None, median::<u8, _>(vec![]));
        assert_eq!(Some(2), median(vec![2u8, 3, 1]));
        assert_eq!(Some(2), median(vec![2u8, 3, 4, 1]));
        assert_eq!(Some(1), median(vec![1u8, 3, 4, 1]));
        assert_eq!(Some(3), median(vec![2u8, 3, 4, 1, 5]));
        assert_eq!(Some(2), median(vec![2u8, 3, 4, 1, 2]));
        assert_eq!(Some(3), median(vec![2u8, 3, 4, 1, 3]));
    }
}
