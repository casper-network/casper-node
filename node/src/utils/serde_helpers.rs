use num::Integer;
use num_rational::Ratio;
use serde::{de, Deserialize, Deserializer};

pub(crate) fn sorted_vec_deserializer<'de, T, D>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    T: Deserialize<'de> + Ord,
    D: Deserializer<'de>,
{
    let mut vec = Vec::<T>::deserialize(deserializer)?;
    vec.sort_unstable();
    Ok(vec)
}

const NUMER_GREATER_THAN_DENOM_MSG: &str = "numerator is greater than the denominator";

/// Deserializes a `Ratio` whose numerator is less or of lower degree than the denominator.
pub(crate) fn proper_fraction_deserializer<'de, T, D>(deserializer: D) -> Result<Ratio<T>, D::Error>
where
    T: Deserialize<'de> + Clone + PartialEq + Integer,
    D: Deserializer<'de>,
{
    let ratio: Ratio<T> = <Ratio<T>>::deserialize(deserializer)?;
    if ratio.numer() > ratio.denom() {
        return Err(de::Error::custom(NUMER_GREATER_THAN_DENOM_MSG));
    }
    Ok(ratio)
}

#[cfg(test)]
mod tests {
    use std::vec;

    use num::One;
    use serde::de::{
        value::{Error as ValueError, SeqDeserializer},
        IntoDeserializer,
    };

    use super::*;

    #[test]
    fn should_not_deserialize_numer_greater_than_denominator() {
        let deserializer_1: SeqDeserializer<vec::IntoIter<u64>, ValueError> =
            vec![150u64, 100u64].into_deserializer();
        let error = proper_fraction_deserializer::<'_, u64, _>(deserializer_1).unwrap_err();
        assert_eq!(error.to_string(), NUMER_GREATER_THAN_DENOM_MSG);

        let deserializer_2: SeqDeserializer<vec::IntoIter<u64>, ValueError> =
            vec![101u64, 100u64].into_deserializer();
        let error = proper_fraction_deserializer::<'_, u64, _>(deserializer_2).unwrap_err();
        assert_eq!(error.to_string(), NUMER_GREATER_THAN_DENOM_MSG);
    }

    #[test]
    fn should_not_deserialize_invalid_fraction() {
        let deserializer: SeqDeserializer<vec::IntoIter<u64>, ValueError> =
            vec![u64::MAX, 0u64].into_deserializer();
        let error = proper_fraction_deserializer::<'_, u64, _>(deserializer).unwrap_err();
        assert_eq!(
            error.to_string(),
            "invalid value: integer `0`, expected a ratio with non-zero denominator"
        );
    }

    #[test]
    fn should_deserialize_proper_fraction() {
        let deserializer_1: SeqDeserializer<vec::IntoIter<u64>, ValueError> =
            vec![1u64, 100u64].into_deserializer();
        let ratio: Ratio<u64> = proper_fraction_deserializer(deserializer_1).unwrap();
        assert_eq!(ratio, Ratio::new_raw(1, 100));

        let deserializer_2: SeqDeserializer<vec::IntoIter<u64>, ValueError> =
            vec![100u64, 100u64].into_deserializer();
        let ratio: Ratio<u64> = proper_fraction_deserializer(deserializer_2).unwrap();
        assert_eq!(ratio, Ratio::one());
    }
}
