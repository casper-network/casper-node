use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use casper_types::EraId;

/// A specification for a stopping point.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum StopAtSpec {
    /// Stop after completion of the current block.
    NextBlock,
    /// Stop after the completion of the next switch block.
    NextEra,
    /// Stop immediately.
    Immediately,
    /// Stop at a given block height.
    BlockHeight(u64),
    /// Stop at a given era id.
    EraId(EraId),
}

impl Default for StopAtSpec {
    fn default() -> Self {
        StopAtSpec::NextBlock
    }
}

impl Display for StopAtSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StopAtSpec::NextBlock => f.write_str("block:next"),
            StopAtSpec::NextEra => f.write_str("era:next"),
            StopAtSpec::Immediately => f.write_str("now"),
            StopAtSpec::BlockHeight(height) => write!(f, "block:{}", height),
            StopAtSpec::EraId(era_id) => write!(f, "era:{}", era_id.value()),
        }
    }
}

impl FromStr for StopAtSpec {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "block:next" => Ok(StopAtSpec::NextBlock),
            "era:next" => Ok(StopAtSpec::NextEra),
            "now" => Ok(StopAtSpec::Immediately),
            val if val.starts_with("block:") => u64::from_str(&val[6..])
                .map_err(|err| format!("could not parse block height: {}", err))
                .map(StopAtSpec::BlockHeight),
            val if val.starts_with("era:") => u64::from_str(&val[4..])
                .map_err(|err| format!("could not parse era id: {}", err))
                .map(EraId::new)
                .map(StopAtSpec::EraId),
            _ => Err("invalid stop-at specification".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::StopAtSpec;
    use casper_types::EraId;
    use proptest::{
        prelude::any,
        prop_oneof, proptest,
        strategy::{Just, Strategy},
    };

    /// A proptest strategy to generate `StopAtSpec` values.
    fn stop_at_strategy() -> impl Strategy<Value = StopAtSpec> {
        prop_oneof![
            Just(StopAtSpec::NextBlock),
            Just(StopAtSpec::NextEra),
            Just(StopAtSpec::Immediately),
            any::<u64>().prop_map(StopAtSpec::BlockHeight),
            any::<u64>().prop_map(|v| StopAtSpec::EraId(EraId::new(v)))
        ]
    }

    proptest! {
        #[test]
        fn roundtrip_stop_at_spec(stop_at in stop_at_strategy()) {
            let rendered = stop_at.to_string();
            let parsed = StopAtSpec::from_str(rendered.as_str()).expect("failed to roundtrip");
            assert_eq!(stop_at, parsed);
        }

        #[test]
        fn string_fuzz_stop_at_spec(input in ".*") {
            let outcome = StopAtSpec::from_str(&input);
            println!("{:?}", outcome);
        }

        #[test]
        fn prefixed_examples(input in "(era|block):.*") {
            let outcome = StopAtSpec::from_str(&input);
            println!("{:?}", outcome);
            // Ensuring it does not crash from parsing.
        }
    }

    #[test]
    fn known_good_examples() {
        assert_eq!(
            Ok(StopAtSpec::NextBlock),
            StopAtSpec::from_str("block:next")
        );
        assert_eq!(Ok(StopAtSpec::NextEra), StopAtSpec::from_str("era:next"));
        assert_eq!(Ok(StopAtSpec::Immediately), StopAtSpec::from_str("now"));
        assert_eq!(
            Ok(StopAtSpec::BlockHeight(123)),
            StopAtSpec::from_str("block:123")
        );
        assert_eq!(
            Ok(StopAtSpec::EraId(EraId::new(123))),
            StopAtSpec::from_str("era:123")
        );
    }
}
