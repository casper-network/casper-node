mod specimen_support {
    use std::collections::BTreeMap;

    use casper_types::{
        bytesrepr::Bytes,
        global_state::{Pointer, TrieMerkleProof, TrieMerkleProofStep},
        CLValue, Digest, Key, StoredValue,
    };

    use crate::{
        contract_runtime::{APPROVALS_CHECKSUM_NAME, EXECUTION_RESULTS_CHECKSUM_NAME},
        utils::specimen::{
            largest_variant, vec_of_largest_specimen, vec_prop_specimen, Cache, LargestSpecimen,
            SizeEstimator,
        },
    };
    use casper_storage::block_store::types::ApprovalsHashes;

    impl LargestSpecimen for ApprovalsHashes {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            let data = {
                let mut map = BTreeMap::new();
                map.insert(
                    APPROVALS_CHECKSUM_NAME,
                    Digest::largest_specimen(estimator, cache),
                );
                map.insert(
                    EXECUTION_RESULTS_CHECKSUM_NAME,
                    Digest::largest_specimen(estimator, cache),
                );
                map
            };
            let merkle_proof_approvals = TrieMerkleProof::new(
                Key::ChecksumRegistry,
                StoredValue::CLValue(CLValue::from_t(data).expect("a correct cl value")),
                // 2^64/2^13 = 2^51, so 51 items:
                vec_of_largest_specimen(estimator, 51, cache).into(),
            );
            ApprovalsHashes::V2 {
                block_hash: LargestSpecimen::largest_specimen(estimator, cache),
                approvals_hashes: vec_prop_specimen(estimator, "approvals_hashes", cache),
                merkle_proof_approvals,
            }
        }
    }

    impl LargestSpecimen for TrieMerkleProofStep {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            #[derive(strum::EnumIter)]
            enum TrieMerkleProofStepDiscriminants {
                Node,
                Extension,
            }

            largest_variant(estimator, |variant| match variant {
                TrieMerkleProofStepDiscriminants::Node => TrieMerkleProofStep::Node {
                    hole_index: u8::MAX,
                    indexed_pointers_with_hole: vec![
                        (
                            u8::MAX,
                            Pointer::LeafPointer(LargestSpecimen::largest_specimen(
                                estimator, cache
                            ))
                        );
                        estimator.parameter("max_pointer_per_node")
                    ],
                },
                TrieMerkleProofStepDiscriminants::Extension => TrieMerkleProofStep::Extension {
                    affix: Bytes::from(vec![u8::MAX; Key::max_serialized_length()]),
                },
            })
        }
    }
}
