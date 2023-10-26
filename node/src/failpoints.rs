//! Failpoint support.
//!
//! Failpoints can enabled on the node to inject faulty behavior at runtime, for testing and
//! benchmarking purposes.

use std::fmt::{self, Display};

use serde_json::Value;

/// A parsed failpoint definition.
///
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FailpointSetting {
    key: String,
    subkey: Option<String>,
    value: Option<Value>,
    probability: Option<f32>,
    once: bool,
}

impl Display for FailpointSetting {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key)?;

        if let Some(subkey) = self.subkey.as_ref() {
            write!(f, ",sub={}", subkey)?;
        }

        if let Some(p) = self.probability {
            write!(f, ",p={}", p)?;
        }

        if self.once {
            f.write_str(",once")?;
        }

        if let Some(value) = self.value.as_ref() {
            // Note on the unwrap: Serializing a `Value` should never fail.
            write!(f, ":{}", serde_json::to_string(value).unwrap_or_default())?;
        }

        Ok(())
    }
}

impl FailpointSetting {
    /// Creates a new [`FailpointSetting`] with the given `key`.
    #[inline(always)]
    pub fn new<S: ToString>(key: S) -> FailpointSetting {
        FailpointSetting {
            key: key.to_string(),
            subkey: None,
            value: None,
            probability: None,
            once: false,
        }
    }

    /// Sets the subkey.
    #[inline(always)]
    pub fn subkey<S: ToString>(mut self, subkey: S) -> Self {
        self.subkey = Some(subkey.to_string());
        self
    }

    /// Sets value from JSON.
    ///
    /// Will parse `value` using a JSON parser.
    ///
    /// # Panics
    ///
    /// Will panic if `value` does not parse into a [`serde_json::Value`].
    #[inline(always)]
    #[cfg(test)]
    pub(crate) fn value_json(self, value: &str) -> Self {
        let value = serde_json::from_str(value).expect("could not parse JSON value");
        self.value(value)
    }

    /// Sets the failpoint's value.
    #[inline(always)]
    pub(crate) fn value(mut self, value: Value) -> Self {
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

    /// Parse a failpoint from a string definition.
    ///
    /// Failpoint syntax is as follows: `key(,meta=meta_value)*:value`, with `key` being the
    /// identifier of the failpoint, `meta` being additional settings, and `value` JSON encoded.
    ///
    /// The following `meta` values are understood:
    ///
    /// * `sub` sets the subkey (example: `sub=e4c2a1f`)
    /// * `p` sets the probability, must be between `0.0` and `1.0` (example: `p=0.1`)
    /// * `once` has no value and indicates the failpoint should only be fired once.
    ///
    /// No colons or commas are allowed in `key`, `meta` or `meta_value`.
    ///
    /// Examples:
    ///
    /// * `foobar` sets the failpoint with key "foobar".
    /// * `foobar,sub=example value,p=0.123,once={"hello": "world"}` sets the failpoint "foobar",
    ///    with a subkey of "example value", a probability of 12.3%, to be fired only once, and a
    ///    JSON encoded value of `{"hello": "world"}`.
    pub(crate) fn parse(raw: &str) -> Option<Self> {
        let (raw_meta, value) = if let Some((left, right)) = raw.split_once(':') {
            (left, Some(serde_json::from_str::<Value>(right).ok()?))
        } else {
            (raw, None)
        };

        let mut fragments = raw_meta.split(',');
        let key = fragments.next()?;
        let mut fps = FailpointSetting::new(key);

        for fragment in fragments {
            let (meta, meta_value) = if let Some((left, right)) = fragment.split_once('=') {
                (left, Some(right))
            } else {
                (fragment, None)
            };

            match (meta, meta_value) {
                ("sub", Some(v)) => {
                    fps = fps.subkey(v);
                }
                ("p", Some(raw_p)) => {
                    fps = fps.probability(raw_p.parse().ok()?);
                }
                ("once", None) => {
                    fps = fps.once();
                }
                _ => return None,
            }
        }

        if let Some(value) = value {
            fps = fps.value(value);
        }

        Some(fps)
    }
}

#[cfg(test)]
mod tests {
    use super::FailpointSetting;

    #[test]
    fn parse_failpoints() {
        assert_eq!(
            FailpointSetting::parse("foobar").expect("should parse"),
            FailpointSetting {
                key: "foobar".to_owned(),
                subkey: None,
                value: None,
                probability: None,
                once: false
            }
        );

        assert_eq!(
            FailpointSetting::parse("foobar,once").expect("should parse"),
            FailpointSetting {
                key: "foobar".to_owned(),
                subkey: None,
                value: None,
                probability: None,
                once: true
            }
        );

        assert_eq!(
            FailpointSetting::parse("foobar,sub=xyz").expect("should parse"),
            FailpointSetting {
                key: "foobar".to_owned(),
                subkey: Some("xyz".to_owned()),
                value: None,
                probability: None,
                once: false
            }
        );

        assert_eq!(
            FailpointSetting::parse("foobar,p=0.5,sub=xyz,once").expect("should parse"),
            FailpointSetting {
                key: "foobar".to_owned(),
                subkey: Some("xyz".to_owned()),
                value: None,
                probability: Some(0.5),
                once: true
            }
        );

        assert_eq!(
            FailpointSetting::parse("foobar,p=0.5,sub=xyz,once:true").expect("should parse"),
            FailpointSetting {
                key: "foobar".to_owned(),
                subkey: Some("xyz".to_owned()),
                value: Some(serde_json::json!(true)),
                probability: Some(0.5),
                once: true
            }
        );

        assert_eq!(
            FailpointSetting::parse("foobar:{\"hello\": \"world\", \"count\": 1}")
                .expect("should parse"),
            FailpointSetting {
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
            FailpointSetting::new("test").probability(-0.1).probability,
            Some(0.0)
        );
        assert_eq!(
            FailpointSetting::new("test").probability(0.0).probability,
            Some(0.0)
        );
        assert_eq!(
            FailpointSetting::new("test").probability(0.1).probability,
            Some(0.1)
        );
        assert_eq!(
            FailpointSetting::new("test").probability(0.5).probability,
            Some(0.5)
        );
        assert_eq!(
            FailpointSetting::new("test").probability(0.9).probability,
            Some(0.9)
        );
        assert_eq!(
            FailpointSetting::new("test").probability(1.0).probability,
            Some(1.0)
        );
        assert_eq!(
            FailpointSetting::new("test").probability(1.1).probability,
            Some(1.0)
        );
    }

    #[test]
    fn display_works() {
        assert_eq!(
            FailpointSetting::parse("foobar:{\"hello\": \"world\", \"count\": 1}")
                .expect("should parse")
                .to_string(),
            "foobar:{\"hello\":\"world\",\"count\":1}"
        );

        assert_eq!(
            FailpointSetting::parse("foobar,p=0.5,sub=xyz,once:true")
                .expect("should parse")
                .to_string(),
            "foobar,sub=xyz,p=0.5,once:true"
        );

        assert_eq!(
            FailpointSetting::parse("abc_123")
                .expect("should parse")
                .to_string(),
            "abc_123"
        );
    }
}
