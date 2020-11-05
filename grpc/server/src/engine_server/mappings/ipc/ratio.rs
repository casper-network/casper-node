use num_rational::Ratio;

use crate::engine_server::ipc;

impl From<ipc::Ratio> for Ratio<u64> {
    fn from(pb_ratio: ipc::Ratio) -> Self {
        Ratio::new(pb_ratio.numer, pb_ratio.denom)
    }
}

impl From<Ratio<u64>> for ipc::Ratio {
    fn from(ratio: Ratio<u64>) -> Self {
        ipc::Ratio {
            numer: *ratio.numer(),
            denom: *ratio.denom(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use num_rational::Ratio;
    use proptest::prelude::*;

    use super::*;
    use crate::engine_server::mappings::test_utils;

    proptest! {
        #[test]
        fn round_trip((numer, denom) in (any::<u64>(), 1..u64::max_value())) {
            let ratio = Ratio::new(numer, denom);
            test_utils::protobuf_round_trip::<_, ipc::Ratio>(ratio);
        }
    }
}
