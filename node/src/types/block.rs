mod approvals_hashes;
mod block_execution_results_or_chunk;
mod block_execution_results_or_chunk_id;
mod block_payload;
mod block_with_metadata;
mod executable_block;
mod finalized_block;
mod meta_block;
mod signed_block;

use casper_types::{
    bytesrepr::{self, ToBytes},
    BlockHash, DeployId, Digest, FinalitySignature, SingleBlockRewardedSignatures,
};

pub(crate) use approvals_hashes::ApprovalsHashes;
pub use block_execution_results_or_chunk::BlockExecutionResultsOrChunk;
pub(crate) use block_execution_results_or_chunk_id::BlockExecutionResultsOrChunkId;
pub(crate) use block_payload::BlockPayload;
pub(crate) use block_with_metadata::BlockWithMetadata;
pub use executable_block::ExecutableBlock;
pub use finalized_block::{FinalizedBlock, InternalEraReport};
pub(crate) use meta_block::{
    ForwardMetaBlock, MergeMismatchError as MetaBlockMergeError, MetaBlock, State as MetaBlockState,
};
pub use signed_block::SignedBlock;

#[cfg_attr(doc, aquamarine::aquamarine)]
/// ```mermaid
/// flowchart TD
///     style Start fill:#66ccff,stroke:#333,stroke-width:4px
///     style End fill:#66ccff,stroke:#333,stroke-width:4px
///     style A fill:#ffcc66,stroke:#333,stroke-width:4px
///     style B fill:#ffcc66,stroke:#333,stroke-width:4px
///     style Q fill:#ADD8E6,stroke:#333,stroke-width:4px
///     style S fill:#ADD8E6,stroke:#333,stroke-width:4px
///     title[FinalitySignature lifecycle]
///     title---Start
///     style title fill:#FFF,stroke:#FFF
///     linkStyle 0 stroke-width:0;
///     Start --> A["Validators"]
///     Start --> B["Non-validators"]
///     A --> C["Validator creates FS"]
///     A --> D["Received</br>broadcasted FS"]
///     A --> E["Received</br>gossiped FS"]
///     D --> I
///     E --> I
///     H --> End
///     C --> G["Put FS to storage"]
///     G --> H["Broadcast FS to Validators"]
///     G --> I["Register FS</br>in BlockAccumulator"]
///     I --> J{"Has sufficient</br>finality</br>and block?"}
///     J --> |Yes| K["Put all FS</br>to storage"]
///     J --> |No| L["Keep waiting</br>for more</br>signatures"]
///     B --> F["Keeping up with</br>the network"]
///     F --> M["Received</br>gossiped FS"]
///     M --> N["Register FS</br>in BlockAccumulator"]
///     N --> O{"Has sufficient</br>finality</br>and block?"}
///     O --> |No| L
///     O --> |Yes| P["Put all FS</br>to storage"]
///     P --> Q["Initiate <b>forward</b></br>sync process</br><i>(click)</i>"]
///     Q --> R["If forward or historical sync</br>process fetched and</br>stored additional FS</br>register them in</br>BlockAccumulator"]
///     B --> S["Initiate <b>historical</b></br>sync process</br><i>(click)</i>"]
///     S --> R
///     click Q "../components/block_synchronizer/block_acquisition/enum.BlockAcquisitionState.html"
///     click S "../components/block_synchronizer/block_acquisition/enum.BlockAcquisitionState.html"
///     R --> End
///     K --> End
/// ```
#[allow(dead_code)]
type ValidatorFinalitySignature = FinalitySignature;

/// Returns the hash of the bytesrepr-encoded deploy_ids.
pub(crate) fn compute_approvals_checksum(
    deploy_ids: Vec<DeployId>,
) -> Result<Digest, bytesrepr::Error> {
    let bytes = deploy_ids.into_bytes()?;
    Ok(Digest::hash(bytes))
}

/// Creates a new recorded finality signatures, from a validator matrix, and a block
/// with metadata.
pub(crate) fn create_single_block_rewarded_signatures(
    validator_matrix: &super::ValidatorMatrix,
    past_block_with_metadata: &BlockWithMetadata,
) -> Option<SingleBlockRewardedSignatures> {
    validator_matrix
        .validator_weights(past_block_with_metadata.block.era_id())
        .map(|weights| {
            SingleBlockRewardedSignatures::from_validator_set(
                &past_block_with_metadata
                    .block_signatures
                    .signers()
                    .cloned()
                    .collect(),
                weights.validator_public_keys(),
            )
        })
}
