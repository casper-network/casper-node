//! Failpoint support.
//!
//! Failpoints can enabled on the node to inject faulty behavior at runtime, for testing and
//! benchmarking purposes.
//!
//! # General usage
//!
//! Failpoints are created in code using `Failpoint`, and activated using a `FailpointActivation`.
//! See the `failpoints::test::various_usecases` test for an example.

use std::{
    fmt::{self, Debug, Display},
    num::ParseFloatError,
    str::FromStr,
};

use datasize::DataSize;
use rand::{distributions::Uniform, prelude::Distribution, Rng};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use thiserror::Error;
use tracing::{info, instrument, trace, warn};

use crate::utils::opt_display::OptDisplay;

/// A specific failpoint.
#[derive(DataSize, Debug)]
pub(crate) struct Failpoint<T>
where
    T: DataSize,
{
    /// Key that activates the given failpoint.
    #[data_size(skip)]
    key: &'static str,
    /// Subkey that potentially activates the given failpoint.
    subkey: Option<String>,
    /// The value of the failpoint, if any.
    value: Option<T>,
    /// Activation probability.
    probability: Option<f32>,
    /// Whether to trigger the failpoint only once.
    once: bool,
    /// Whether the failpoint has already fired.
    fired: bool,
}

impl<T> Failpoint<T>
where
    T: Debug + DeserializeOwned + DataSize,
{
    /// Creates a new failpoint with a given key.
    #[inline(always)]
    pub(crate) fn new(key: &'static str) -> Self {
        Failpoint {
            key,
            subkey: None,
            value: None,
            probability: None,
            once: false,
            fired: false,
        }
    }

    /// Creates a new failpoint with a given key and optional subkey.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn new_with_subkey<S: ToString>(key: &'static str, subkey: S) -> Self {
        Failpoint {
            key,
            subkey: Some(subkey.to_string()),
            value: None,
            probability: None,
            once: false,
            fired: false,
        }
    }

    /// Update a failpoint from a given `FailpointActivation`.
    ///
    /// The failpoint will be changed if the given activation matches `key` and `subkey` only.
    #[instrument(level = "error",
                 fields(fp_key=self.key,
                        fp_subkey=%OptDisplay::new(self.subkey.as_ref(), "")
                       )
                )]
    pub(crate) fn update_from(&mut self, activation: &FailpointActivation) {
        // Check if the failpoint matches.
        if activation.key != self.key || activation.subkey != self.subkey {
            trace!("not updating failpoint");
            return;
        }

        // Values can fail, so update these first.
        if let Some(value) = activation.value.as_ref() {
            match serde_json::from_value::<T>(value.clone()) {
                Ok(value) => self.value = Some(value),
                Err(err) => warn!(%err, "failed to deserialize failpoint value"),
            }
        } else {
            self.value = None;
        }

        self.probability = activation.probability;
        self.once = activation.once;
        self.fired = false;

        if self.value.is_some() {
            info!("activated failpoint");
        } else {
            info!("cleared failpoint");
        }
    }

    /// Fire the failpoint, if active.
    ///
    /// Returns the value of the failpoint, if it fired.
    #[inline(always)]
    pub(crate) fn fire<R: Rng>(&mut self, rng: &mut R) -> Option<&T> {
        if self.value.is_some() {
            self.do_fire(rng)
        } else {
            None
        }
    }

    /// Inner `fire` implementation.
    ///
    /// `fire` is kept small for facilitate inlining and fast processing of disabled failpoints.
    #[inline]
    fn do_fire<R: Rng>(&mut self, rng: &mut R) -> Option<&T> {
        if let Some(p) = self.probability {
            let p_range = Uniform::new_inclusive(0.0, 1.0);
            if p_range.sample(rng) > p as f64 {
                return None;
            }
        }

        if self.once && self.fired {
            return None;
        }

        self.fired = true;
        self.value()
    }

    /// Returns the value of the failpoint, if it is set.
    #[inline]
    fn value(&self) -> Option<&T> {
        self.value.as_ref()
    }
}

/// A parsed failpoint activation.
#[derive(Clone, DataSize, Debug, PartialEq, Serialize)]
pub(crate) struct FailpointActivation {
    key: String,
    subkey: Option<String>,
    #[data_size(skip)] // TODO: Add a `DataSize` implementation for JSON `Value`s.
    value: Option<Value>,
    probability: Option<f32>,
    once: bool,
}

impl Display for FailpointActivation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key)?;

        if let Some(subkey) = self.subkey.as_ref() {
            write!(f, ",sub:{}", subkey)?;
        }

        if let Some(p) = self.probability {
            write!(f, ",p:{}", p)?;
        }

        if self.once {
            f.write_str(",once")?;
        }

        if let Some(value) = self.value.as_ref() {
            // Note on the unwrap: Serializing a `Value` should never fail.
            write!(f, "={}", serde_json::to_string(value).unwrap_or_default())?;
        }

        Ok(())
    }
}

impl FailpointActivation {
    /// Creates a new [`FailpointActivation`] with the given `key`.
    #[inline(always)]
    pub(crate) fn new<S: ToString>(key: S) -> FailpointActivation {
        FailpointActivation {
            key: key.to_string(),
            subkey: None,
            value: None,
            probability: None,
            once: false,
        }
    }

    /// Gets the key of this [`FailpointActivation`].
    #[inline(always)]
    pub(crate) fn key(&self) -> &str {
        &self.key
    }

    /// Sets the subkey.
    #[inline(always)]
    pub(crate) fn subkey<S: ToString>(mut self, subkey: S) -> Self {
        self.subkey = Some(subkey.to_string());
        self
    }

    /// Sets the failpoint's value from JSON.
    ///
    /// # Panics
    ///
    /// Will panic if `value` does not cleanly serialize to a [`serde_json::Value`].
    #[inline(always)]
    #[allow(unused)]
    pub(crate) fn value<T>(self, value: T) -> Self
    where
        T: Serialize,
    {
        let value_json: Value =
            serde_json::to_value(value).expect("passed in value does not serialize to JSON");

        self.value_json(value_json)
    }

    /// Sets the failpoint's value from JSON.
    #[inline(always)]
    pub(crate) fn value_json(mut self, value: Value) -> Self {
        self.value = Some(value);
        self
    }

    /// Sets the probability of the failpoint firing.
    ///
    /// The value will be clamped to `[0.0, 1.0]`. A value of `NaN` will be converted to `0.0`.
    #[inline(always)]
    pub(crate) fn probability(mut self, probability: f32) -> Self {
        // Note: We do not use `clamp`, since it does not remove `NaN`s.
        self.probability = Some(probability.max(0.0).min(1.0));
        self
    }

    /// Sets the failpoint to fire only once.
    #[inline(always)]
    pub(crate) fn once(mut self) -> Self {
        self.once = true;
        self
    }
}

/// Error parsing a failpoint activation.
#[derive(Debug, Error)]
pub(crate) enum ParseError {
    /// The provided value for the failpoint was not valid JSON.
    #[error("invalid json value")]
    InvalidJson(#[source] serde_json::Error),
    /// Left hand side contained no segments.
    #[error("no key given")]
    MissingKey,
    /// Invalid floating literal for probability
    #[error("invvalid probability value")]
    InvalidProbability(#[source] ParseFloatError),
    /// The given meta key is not valid.
    #[error("not a known meta key: \"{0}\"")]
    InvalidMeta(String),
}

impl FromStr for FailpointActivation {
    type Err = ParseError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let (raw_meta, value) = if let Some((left, right)) = raw.split_once('=') {
            (
                left,
                Some(serde_json::from_str::<Value>(right).map_err(ParseError::InvalidJson)?),
            )
        } else {
            (raw, None)
        };

        let mut fragments = raw_meta.split(',');
        let key = fragments.next().ok_or(ParseError::MissingKey)?;
        let mut fps = FailpointActivation::new(key);

        for fragment in fragments {
            let (meta, meta_value) = if let Some((left, right)) = fragment.split_once(':') {
                (left, Some(right))
            } else {
                (fragment, None)
            };

            match (meta, meta_value) {
                ("sub", Some(v)) => {
                    fps = fps.subkey(v);
                }
                ("p", Some(raw_p)) => {
                    fps = fps.probability(raw_p.parse().map_err(ParseError::InvalidProbability)?);
                }
                ("once", None) => {
                    fps = fps.once();
                }
                (invalid, _) => return Err(ParseError::InvalidMeta(invalid.to_string())),
            }
        }

        if let Some(value) = value {
            fps = fps.value_json(value);
        }

        Ok(fps)
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use casper_types::{testing::TestRng, TimeDiff};
    use serde_json::json;

    use crate::testing::init_logging;

    use super::{Failpoint, FailpointActivation};

    #[test]
    fn parse_failpoints() {
        assert_eq!(
            FailpointActivation::from_str("foobar").expect("should parse"),
            FailpointActivation {
                key: "foobar".to_owned(),
                subkey: None,
                value: None,
                probability: None,
                once: false
            }
        );

        assert_eq!(
            FailpointActivation::from_str("foobar,once").expect("should parse"),
            FailpointActivation {
                key: "foobar".to_owned(),
                subkey: None,
                value: None,
                probability: None,
                once: true
            }
        );

        assert_eq!(
            FailpointActivation::from_str("foobar,sub:xyz").expect("should parse"),
            FailpointActivation {
                key: "foobar".to_owned(),
                subkey: Some("xyz".to_owned()),
                value: None,
                probability: None,
                once: false
            }
        );

        assert_eq!(
            FailpointActivation::from_str("foobar,p:0.5,sub:xyz,once").expect("should parse"),
            FailpointActivation {
                key: "foobar".to_owned(),
                subkey: Some("xyz".to_owned()),
                value: None,
                probability: Some(0.5),
                once: true
            }
        );

        assert_eq!(
            FailpointActivation::from_str("foobar,p:0.5,sub:xyz,once=true").expect("should parse"),
            FailpointActivation {
                key: "foobar".to_owned(),
                subkey: Some("xyz".to_owned()),
                value: Some(serde_json::json!(true)),
                probability: Some(0.5),
                once: true
            }
        );

        assert_eq!(
            FailpointActivation::from_str("foobar={\"hello\": \"world\", \"count\": 1}")
                .expect("should parse"),
            FailpointActivation {
                key: "foobar".to_owned(),
                subkey: None,
                value: Some(serde_json::json!({"hello": "world", "count": 1})),
                probability: None,
                once: false
            }
        );
    }

    #[test]
    fn clamping_works() {
        assert_eq!(
            FailpointActivation::new("test")
                .probability(-0.1)
                .probability,
            Some(0.0)
        );
        assert_eq!(
            FailpointActivation::new("test")
                .probability(0.0)
                .probability,
            Some(0.0)
        );
        assert_eq!(
            FailpointActivation::new("test")
                .probability(0.1)
                .probability,
            Some(0.1)
        );
        assert_eq!(
            FailpointActivation::new("test")
                .probability(0.5)
                .probability,
            Some(0.5)
        );
        assert_eq!(
            FailpointActivation::new("test")
                .probability(0.9)
                .probability,
            Some(0.9)
        );
        assert_eq!(
            FailpointActivation::new("test")
                .probability(1.0)
                .probability,
            Some(1.0)
        );
        assert_eq!(
            FailpointActivation::new("test")
                .probability(1.1)
                .probability,
            Some(1.0)
        );
    }

    #[test]
    fn display_works() {
        assert_eq!(
            FailpointActivation::from_str("foobar={\"hello\": \"world\", \"count\": 1}")
                .expect("should parse")
                .to_string(),
            "foobar={\"hello\":\"world\",\"count\":1}"
        );

        assert_eq!(
            FailpointActivation::from_str("foobar,p:0.5,sub:xyz,once=true")
                .expect("should parse")
                .to_string(),
            "foobar,sub:xyz,p:0.5,once=true"
        );

        assert_eq!(
            FailpointActivation::from_str("abc_123")
                .expect("should parse")
                .to_string(),
            "abc_123"
        );
    }

    #[test]
    fn various_usecases() {
        // Note: This function deliberately exerts different APIs of `FailpointActivation`. When
        //       using `FailpointActivation` in tests, it is recommend to construct it using the
        //       builder pattern as opposed to parsing it from strings.

        init_logging();

        let mut rng = TestRng::new();
        let mut delay_send_fp = Failpoint::<TimeDiff>::new("example.delay_send");

        assert!(
            delay_send_fp.fire(&mut rng).is_none(),
            "failpoint should be disabled"
        );

        let unrelated_activation =
            FailpointActivation::from_str("example.unrelated=\"1s\"").unwrap();
        delay_send_fp.update_from(&unrelated_activation);

        assert!(
            delay_send_fp.fire(&mut rng).is_none(),
            "failpoint should be disabled after unrelated activation"
        );

        let activation =
            FailpointActivation::new("example.delay_send").value(TimeDiff::from_seconds(1));

        delay_send_fp.update_from(&activation);

        let diff = delay_send_fp
            .fire(&mut rng)
            .expect("should trigger failpoint");
        assert_eq!(*diff, TimeDiff::from_str("1s").unwrap());

        // Repeat, since `once` is not enabled.
        let diff = delay_send_fp
            .fire(&mut rng)
            .expect("should trigger failpoint a second time");
        assert_eq!(*diff, TimeDiff::from_str("1s").unwrap());
        let diff = delay_send_fp
            .fire(&mut rng)
            .expect("should trigger failpoint a third time");
        assert_eq!(*diff, TimeDiff::from_str("1s").unwrap());

        let deactivation = FailpointActivation::from_str("example.delay_send").unwrap();

        delay_send_fp.update_from(&deactivation);

        assert!(
            delay_send_fp.fire(&mut rng).is_none(),
            "failpoint should be disabled"
        );

        let once_activation = FailpointActivation::new("example.delay_send")
            .once()
            .value_json(json!("2s"));
        delay_send_fp.update_from(&once_activation);

        let diff = delay_send_fp
            .fire(&mut rng)
            .expect("should trigger failpoint");
        assert_eq!(*diff, TimeDiff::from_str("2s").unwrap());

        // Repeat failpoint triggered once should not fire again.
        assert!(delay_send_fp.fire(&mut rng).is_none());
    }

    #[test]
    fn activation_primes_properly() {
        let mut fp = Failpoint::<()>::new("some_failpoint");

        fp.update_from(&FailpointActivation::from_str("some_failpoint,p:0.5,once=null").unwrap());

        assert_eq!(fp.probability, Some(0.5));
        assert!(fp.once);
    }

    #[test]
    fn failpoint_probability_affects_failpoint() {
        let mut rng = TestRng::new();
        let mut fp = Failpoint::<()>::new("some_failpoint");

        // Full activation.
        fp.update_from(&FailpointActivation::from_str("some_failpoint=null").unwrap());
        assert!(fp.fire(&mut rng).is_some());

        // p:1.0 should be the same
        fp.update_from(&FailpointActivation::from_str("some_failpoint,p:1.0=null").unwrap());
        assert!(fp.fire(&mut rng).is_some());

        // p:0.0 essentially disables it
        fp.update_from(&FailpointActivation::from_str("some_failpoint,p:0.0=null").unwrap());
        assert!(fp.fire(&mut rng).is_none());
    }
}
