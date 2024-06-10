use std::{
    fmt::{self, Display, Formatter},
    str::FromStr,
};

use casper_types::EraId;
use datasize::DataSize;
use serde::Serialize;

/// A specification for a stopping point.
#[derive(Copy, Clone, DataSize, Debug, Eq, PartialEq, Serialize, Default)]
#[cfg_attr(test, derive(proptest_derive::Arbitrary))]
pub(crate) enum StopAtSpec {
    /// Stop after completion of the current block.
    #[default]
    NextBlock,
    /// Stop after the completion of the next switch block.
    EndOfCurrentEra,
    /// Stop immediately.
    Immediately,
    /// Stop at a given block height.
    BlockHeight(u64),
    /// Stop at a given era id.
    EraId(EraId),
}

impl Display for StopAtSpec {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            StopAtSpec::NextBlock => f.write_str("block:next"),
            StopAtSpec::EndOfCurrentEra => f.write_str("era:end"),
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
            "era:end" => Ok(StopAtSpec::EndOfCurrentEra),
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
    use proptest::proptest;

    proptest! {
        #[test]
        fn roundtrip_stop_at_spec(stop_at: StopAtSpec) {
            let rendered = stop_at.to_string();
            let parsed = StopAtSpec::from_str(rendered.as_str()).expect("failed to roundtrip");
            assert_eq!(stop_at, parsed);
        }

        #[test]
        fn string_fuzz_stop_at_spec(input in ".*") {
            let _outcome = StopAtSpec::from_str(&input);
        }

        #[test]
        fn prefixed_examples(input in "(era|block):.*") {
            let _outcome = StopAtSpec::from_str(&input);
        }
    }

    #[test]
    fn known_good_examples() {
        assert_eq!(
            Ok(StopAtSpec::NextBlock),
            StopAtSpec::from_str("block:next")
        );
        assert_eq!(
            Ok(StopAtSpec::EndOfCurrentEra),
            StopAtSpec::from_str("era:end")
        );
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
