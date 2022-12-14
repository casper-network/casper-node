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
        return Err("not implemented yet".to_string());
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

    }
}
