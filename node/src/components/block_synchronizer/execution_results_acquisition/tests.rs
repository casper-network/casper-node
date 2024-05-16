use assert_matches::assert_matches;

use casper_types::{
    bytesrepr::ToBytes, execution::ExecutionResultV2, testing::TestRng, DeployHash,
    TestBlockBuilder,
};

use super::*;
use crate::{
    components::block_synchronizer::tests::test_utils::chunks_with_proof_from_data,
    types::BlockExecutionResultsOrChunkId,
};

const NUM_TEST_EXECUTION_RESULTS: u64 = 100000;

#[test]
fn execution_results_chunks_apply_correctly() {
    let rng = &mut TestRng::new();
    let block = TestBlockBuilder::new().build(rng);

    // Create chunkable execution results
    let exec_results: Vec<ExecutionResult> = (0..NUM_TEST_EXECUTION_RESULTS)
        .map(|_| ExecutionResult::from(ExecutionResultV2::random(rng)))
        .collect();
    let test_chunks = chunks_with_proof_from_data(&exec_results.to_bytes().unwrap());
    assert!(test_chunks.len() >= 3);

    // Start off with only one chunk applied
    let mut chunks: HashMap<u64, ChunkWithProof> = HashMap::new();
    let first_chunk = test_chunks.first_key_value().unwrap();
    let last_chunk = test_chunks.last_key_value().unwrap();
    chunks.insert(*first_chunk.0, first_chunk.1.clone());

    // Insert all the other chunks except the last; skip the first one since it should have been
    // added already
    for (index, chunk) in test_chunks.iter().take(test_chunks.len() - 1).skip(1) {
        let apply_result = apply_chunk(
            *block.hash(),
            ExecutionResultsChecksum::Uncheckable,
            chunks,
            chunk.clone(),
            None,
        );

        // Check the index of the next chunk that should be applied
        chunks = assert_matches!(apply_result, Ok(ApplyChunkOutcome::NeedNext{chunks, chunk_count, next}) => {
            assert_eq!(next, index + 1);
            assert_eq!(chunk_count as usize, test_chunks.len());
            chunks
        });
    }

    // Apply the last chunk, and expect to get back the execution results
    let apply_result = apply_chunk(
        *block.hash(),
        ExecutionResultsChecksum::Uncheckable,
        chunks,
        last_chunk.1.clone(),
        None,
    );
    assert_matches!(apply_result, Ok(ApplyChunkOutcome::Complete{execution_results}) => {
        assert_eq!(execution_results, exec_results);
    });
}

#[test]
fn single_chunk_execution_results_dont_apply_other_chunks() {
    let rng = &mut TestRng::new();
    let test_chunks = chunks_with_proof_from_data(&[0; ChunkWithProof::CHUNK_SIZE_BYTES - 1]);
    assert_eq!(test_chunks.len(), 1);

    // We can't apply a chunk if the execution results are not chunked (only 1 chunk exists)
    // Expect an error in this case.
    let first_chunk = test_chunks.first_key_value().unwrap();

    let apply_result = apply_chunk(
        *TestBlockBuilder::new().build(rng).hash(),
        ExecutionResultsChecksum::Uncheckable,
        test_chunks.clone().into_iter().collect(),
        first_chunk.1.clone(),
        None,
    );

    assert_matches!(apply_result, Err(Error::InvalidChunkCount { .. }));
}

#[test]
fn execution_results_chunks_from_block_with_different_hash_are_not_applied() {
    let rng = &mut TestRng::new();
    let block = TestBlockBuilder::new().build(rng);
    let test_chunks = chunks_with_proof_from_data(&[0; ChunkWithProof::CHUNK_SIZE_BYTES * 3]);

    // Start acquiring chunks
    let mut acquisition = ExecutionResultsAcquisition::new_acquiring(
        *block.hash(),
        ExecutionResultsChecksum::Uncheckable,
        test_chunks.clone().into_iter().take(1).collect(),
        3,
        1,
    );
    let exec_result = BlockExecutionResultsOrChunk::new_from_value(
        *block.hash(),
        ValueOrChunk::ChunkWithProof(test_chunks.last_key_value().unwrap().1.clone()),
    );
    acquisition = assert_matches!(
        acquisition.apply_block_execution_results_or_chunk(exec_result, vec![]),
        Ok((acq, Acceptance::NeededIt)) => acq
    );
    assert_matches!(acquisition, ExecutionResultsAcquisition::Acquiring { .. });

    // Applying execution results from other block should return an error
    let exec_result = BlockExecutionResultsOrChunk::new_from_value(
        *TestBlockBuilder::new().build(rng).hash(),
        ValueOrChunk::ChunkWithProof(test_chunks.first_key_value().unwrap().1.clone()),
    );
    assert_matches!(
        acquisition.apply_block_execution_results_or_chunk(exec_result, vec![]),
        Err(Error::BlockHashMismatch {expected, .. }) => assert_eq!(expected, *block.hash())
    );
}

#[test]
fn execution_results_chunks_from_trie_with_different_chunk_count_are_not_applied() {
    let rng = &mut TestRng::new();
    let test_chunks_1 = chunks_with_proof_from_data(&[0; ChunkWithProof::CHUNK_SIZE_BYTES * 3]);
    assert_eq!(test_chunks_1.len(), 3);

    let test_chunks_2 = chunks_with_proof_from_data(&[1; ChunkWithProof::CHUNK_SIZE_BYTES * 2]);
    assert_eq!(test_chunks_2.len(), 2);

    // If chunk tries have different number of chunks we shouldn't attempt to apply the incoming
    // chunk and exit early
    let bad_chunk = test_chunks_2.first_key_value().unwrap();

    let apply_result = apply_chunk(
        *TestBlockBuilder::new().build(rng).hash(),
        ExecutionResultsChecksum::Uncheckable,
        test_chunks_1.into_iter().take(2).collect(),
        bad_chunk.1.clone(),
        Some(3),
    );

    assert_matches!(apply_result, Err(Error::ChunkCountMismatch {expected, actual, ..}) if expected == 3 && actual == 2);
}

#[test]
fn invalid_execution_results_from_applied_chunks_dont_deserialize() {
    let rng = &mut TestRng::new();
    let block = TestBlockBuilder::new().build(rng);

    // Create some chunk data that cannot pe serialized into execution results
    let test_chunks = chunks_with_proof_from_data(&[0; ChunkWithProof::CHUNK_SIZE_BYTES * 2]);
    assert_eq!(test_chunks.len(), 2);
    let last_chunk = test_chunks.last_key_value().unwrap();

    // Expect that this data cannot be deserialized
    let apply_result = apply_chunk(
        *block.hash(),
        ExecutionResultsChecksum::Uncheckable,
        test_chunks.clone().into_iter().take(1).collect(),
        last_chunk.1.clone(),
        None,
    );
    assert_matches!(apply_result, Err(Error::FailedToDeserialize { .. }));
}

#[test]
fn cant_apply_chunk_from_different_exec_results_or_invalid_checksum() {
    let rng = &mut TestRng::new();
    let block = TestBlockBuilder::new().build(rng);

    // Create valid execution results
    let valid_exec_results: Vec<ExecutionResult> = (0..NUM_TEST_EXECUTION_RESULTS)
        .map(|_| ExecutionResult::from(ExecutionResultV2::random(rng)))
        .collect();
    let valid_test_chunks = chunks_with_proof_from_data(&valid_exec_results.to_bytes().unwrap());
    assert!(valid_test_chunks.len() >= 3);

    // Create some invalid chunks that are not part of the execution results we are building
    let invalid_test_chunks =
        chunks_with_proof_from_data(&[0; ChunkWithProof::CHUNK_SIZE_BYTES * 2]);
    assert_eq!(invalid_test_chunks.len(), 2);

    // Try to apply the invalid test chunks to the valid chunks and expect to fail since the
    // checksums for the proofs are different between the chunks.
    let apply_result = apply_chunk(
        *block.hash(),
        ExecutionResultsChecksum::Uncheckable,
        valid_test_chunks.clone().into_iter().take(2).collect(),
        invalid_test_chunks.first_key_value().unwrap().1.clone(),
        None,
    );
    assert_matches!(apply_result, Err(Error::ChunksWithDifferentChecksum{block_hash: _, expected, actual}) => {
        assert_eq!(expected, valid_test_chunks.first_key_value().unwrap().1.proof().root_hash());
        assert_eq!(actual, invalid_test_chunks.first_key_value().unwrap().1.proof().root_hash());
    });

    // Same test but here we are explicitly specifying the execution results checksum that
    // should be checked.
    let apply_result = apply_chunk(
        *block.hash(),
        ExecutionResultsChecksum::Checkable(
            valid_test_chunks
                .first_key_value()
                .unwrap()
                .1
                .proof()
                .root_hash(),
        ),
        valid_test_chunks.clone().into_iter().take(2).collect(),
        invalid_test_chunks.first_key_value().unwrap().1.clone(),
        None,
    );
    assert_matches!(apply_result, Err(Error::ChecksumMismatch{block_hash: _, expected, actual}) => {
        assert_eq!(expected, valid_test_chunks.first_key_value().unwrap().1.proof().root_hash());
        assert_eq!(actual, invalid_test_chunks.first_key_value().unwrap().1.proof().root_hash());
    });
}

// Constructors for acquisition states used for testing and verifying generic properties of
// these states
impl ExecutionResultsAcquisition {
    fn new_needed(block_hash: BlockHash) -> Self {
        let acq = Self::Needed { block_hash };
        assert_eq!(acq.block_hash(), block_hash);
        assert!(!acq.is_checkable());
        assert_eq!(acq.needs_value_or_chunk(), None);
        acq
    }

    fn new_pending(block_hash: BlockHash, checksum: ExecutionResultsChecksum) -> Self {
        let acq = Self::Pending {
            block_hash,
            checksum,
        };
        assert_eq!(acq.block_hash(), block_hash);
        assert_eq!(acq.is_checkable(), checksum.is_checkable());
        assert_eq!(
            acq.needs_value_or_chunk(),
            Some((BlockExecutionResultsOrChunkId::new(block_hash), checksum))
        );
        acq
    }

    fn new_acquiring(
        block_hash: BlockHash,
        checksum: ExecutionResultsChecksum,
        chunks: HashMap<u64, ChunkWithProof>,
        chunk_count: u64,
        next: u64,
    ) -> Self {
        let acq = Self::Acquiring {
            block_hash,
            checksum,
            chunks,
            chunk_count,
            next,
        };
        assert_eq!(acq.block_hash(), block_hash);
        assert_eq!(acq.is_checkable(), checksum.is_checkable());
        assert_eq!(
            acq.needs_value_or_chunk(),
            Some((
                BlockExecutionResultsOrChunkId::new(block_hash).next_chunk(next),
                checksum
            ))
        );
        acq
    }

    fn new_complete(
        block_hash: BlockHash,
        checksum: ExecutionResultsChecksum,
        results: HashMap<TransactionHash, ExecutionResult>,
    ) -> Self {
        let acq = Self::Complete {
            block_hash,
            checksum,
            results,
        };
        assert_eq!(acq.block_hash(), block_hash);
        assert_eq!(acq.is_checkable(), checksum.is_checkable());
        assert_eq!(acq.needs_value_or_chunk(), None);
        acq
    }
}

#[test]
fn acquisition_needed_state_has_correct_transitions() {
    let rng = &mut TestRng::new();
    let block = TestBlockBuilder::new().build(rng);

    let acquisition = ExecutionResultsAcquisition::new_needed(*block.hash());

    let exec_results_checksum = ExecutionResultsChecksum::Checkable(Digest::hash([0; 32]));
    assert_matches!(
        acquisition.clone().apply_checksum(exec_results_checksum),
        Ok(ExecutionResultsAcquisition::Pending{block_hash, checksum}) if block_hash == *block.hash() && checksum == exec_results_checksum
    );

    assert_matches!(
        acquisition.clone().apply_checksum(ExecutionResultsChecksum::Uncheckable),
        Ok(ExecutionResultsAcquisition::Pending{block_hash, checksum}) if block_hash == *block.hash() && checksum == ExecutionResultsChecksum::Uncheckable
    );

    let mut test_chunks = chunks_with_proof_from_data(&[0; ChunkWithProof::CHUNK_SIZE_BYTES]);
    assert_eq!(test_chunks.len(), 1);

    let exec_result = BlockExecutionResultsOrChunk::new_from_value(
        *block.hash(),
        ValueOrChunk::ChunkWithProof(test_chunks.remove(&0).unwrap()),
    );
    assert_matches!(
        acquisition.apply_block_execution_results_or_chunk(exec_result, vec![]),
        Err(Error::AttemptToApplyDataWhenMissingChecksum { .. })
    );
}

#[test]
fn acquisition_pending_state_has_correct_transitions() {
    let rng = &mut TestRng::new();
    let block = TestBlockBuilder::new().build(rng);

    let acquisition = ExecutionResultsAcquisition::new_pending(
        *block.hash(),
        ExecutionResultsChecksum::Uncheckable,
    );
    assert_matches!(
        acquisition
            .clone()
            .apply_checksum(ExecutionResultsChecksum::Uncheckable),
        Err(Error::InvalidAttemptToApplyChecksum { .. })
    );

    // Acquisition can transition from `Pending` to `Complete` if a value and deploy hashes are
    // applied
    let execution_results = vec![ExecutionResult::from(ExecutionResultV2::random(rng))];
    let exec_result = BlockExecutionResultsOrChunk::new_from_value(
        *block.hash(),
        ValueOrChunk::new(execution_results, 0).unwrap(),
    );
    assert_matches!(
        acquisition
            .clone()
            .apply_block_execution_results_or_chunk(exec_result.clone(), vec![]),
        Err(Error::ExecutionResultToDeployHashLengthDiscrepancy { .. })
    );
    assert_matches!(
        acquisition.clone().apply_block_execution_results_or_chunk(
            exec_result,
            vec![DeployHash::new(Digest::hash([0; 32])).into()]
        ),
        Ok((
            ExecutionResultsAcquisition::Complete { .. },
            Acceptance::NeededIt
        ))
    );

    // Acquisition can transition from `Pending` to `Acquiring` if a single chunk is applied
    let exec_results: Vec<ExecutionResult> = (0..NUM_TEST_EXECUTION_RESULTS)
        .map(|_| ExecutionResult::from(ExecutionResultV2::random(rng)))
        .collect();
    let test_chunks = chunks_with_proof_from_data(&exec_results.to_bytes().unwrap());
    assert!(test_chunks.len() >= 3);

    let first_chunk = test_chunks.first_key_value().unwrap().1;
    let exec_result = BlockExecutionResultsOrChunk::new_from_value(
        *block.hash(),
        ValueOrChunk::ChunkWithProof(first_chunk.clone()),
    );
    let transaction_hashes: Vec<TransactionHash> = (0..NUM_TEST_EXECUTION_RESULTS)
        .map(|index| DeployHash::new(Digest::hash(index.to_bytes().unwrap())).into())
        .collect();
    assert_matches!(
        acquisition.apply_block_execution_results_or_chunk(exec_result, transaction_hashes),
        Ok((
            ExecutionResultsAcquisition::Acquiring { .. },
            Acceptance::NeededIt
        ))
    );
}

#[test]
fn acquisition_acquiring_state_has_correct_transitions() {
    let rng = &mut TestRng::new();
    let block = TestBlockBuilder::new().build(rng);

    // Generate valid execution results that are chunkable
    let exec_results: Vec<ExecutionResult> = (0..NUM_TEST_EXECUTION_RESULTS)
        .map(|_| ExecutionResult::from(ExecutionResultV2::random(rng)))
        .collect();
    let test_chunks = chunks_with_proof_from_data(&exec_results.to_bytes().unwrap());
    assert!(test_chunks.len() >= 3);

    let mut acquisition = ExecutionResultsAcquisition::new_acquiring(
        *block.hash(),
        ExecutionResultsChecksum::Uncheckable,
        test_chunks.clone().into_iter().take(1).collect(),
        test_chunks.len() as u64,
        1,
    );
    assert_matches!(
        acquisition
            .clone()
            .apply_checksum(ExecutionResultsChecksum::Uncheckable),
        Err(Error::InvalidAttemptToApplyChecksum { .. })
    );

    // Apply all chunks except the last and check if the acquisition state remains `Acquiring`
    for (_, chunk) in test_chunks.iter().take(test_chunks.len() - 1).skip(1) {
        let exec_result = BlockExecutionResultsOrChunk::new_from_value(
            *block.hash(),
            ValueOrChunk::ChunkWithProof(chunk.clone()),
        );
        acquisition = assert_matches!(
            acquisition.apply_block_execution_results_or_chunk(exec_result, vec![]),
            Ok((acq, Acceptance::NeededIt)) => acq
        );
        assert_matches!(acquisition, ExecutionResultsAcquisition::Acquiring { .. });
    }

    // Now apply the last chunk and check if the acquisition completes
    let last_chunk = test_chunks.last_key_value().unwrap().1;
    let exec_result = BlockExecutionResultsOrChunk::new_from_value(
        *block.hash(),
        ValueOrChunk::ChunkWithProof(last_chunk.clone()),
    );
    let transaction_hashes: Vec<TransactionHash> = (0..NUM_TEST_EXECUTION_RESULTS)
        .map(|index| DeployHash::new(Digest::hash(index.to_bytes().unwrap())).into())
        .collect();
    acquisition = assert_matches!(
        acquisition.apply_block_execution_results_or_chunk(exec_result, transaction_hashes),
        Ok((acq, Acceptance::NeededIt)) => acq
    );
    assert_matches!(acquisition, ExecutionResultsAcquisition::Complete { .. });
}

#[test]
fn acquisition_acquiring_state_gets_overridden_by_value() {
    let rng = &mut TestRng::new();
    let block = TestBlockBuilder::new().build(rng);
    let test_chunks = chunks_with_proof_from_data(&[0; ChunkWithProof::CHUNK_SIZE_BYTES * 3]);

    // Start acquiring chunks
    let mut acquisition = ExecutionResultsAcquisition::new_acquiring(
        *block.hash(),
        ExecutionResultsChecksum::Uncheckable,
        test_chunks.clone().into_iter().take(1).collect(),
        3,
        1,
    );
    let exec_result = BlockExecutionResultsOrChunk::new_from_value(
        *block.hash(),
        ValueOrChunk::ChunkWithProof(test_chunks.last_key_value().unwrap().1.clone()),
    );
    acquisition = assert_matches!(
        acquisition.apply_block_execution_results_or_chunk(exec_result, vec![]),
        Ok((acq, Acceptance::NeededIt)) => acq
    );
    assert_matches!(acquisition, ExecutionResultsAcquisition::Acquiring { .. });

    // Assume we got a full execution result for this block.
    // Since we don't have a checksum for the execution results, we can't really determine which
    // data is the better one. We expect to overwrite the execution results chunks that
    // we previously acquired with this complete result.
    let execution_results = vec![ExecutionResult::from(ExecutionResultV2::random(rng))];
    let exec_result = BlockExecutionResultsOrChunk::new_from_value(
        *block.hash(),
        ValueOrChunk::new(execution_results, 0).unwrap(),
    );
    assert_matches!(
        acquisition
            .clone()
            .apply_block_execution_results_or_chunk(exec_result.clone(), vec![]),
        Err(Error::ExecutionResultToDeployHashLengthDiscrepancy { .. })
    );

    assert_matches!(
        acquisition.apply_block_execution_results_or_chunk(
            exec_result,
            vec![DeployHash::new(Digest::hash([0; 32])).into()]
        ),
        Ok((
            ExecutionResultsAcquisition::Complete { .. },
            Acceptance::NeededIt
        ))
    );
}

#[test]
fn acquisition_complete_state_has_correct_transitions() {
    let rng = &mut TestRng::new();
    let block = TestBlockBuilder::new().build(rng);

    let acquisition = ExecutionResultsAcquisition::new_complete(
        *block.hash(),
        ExecutionResultsChecksum::Uncheckable,
        HashMap::new(),
    );

    let exec_results_checksum = ExecutionResultsChecksum::Checkable(Digest::hash([0; 32]));
    assert_matches!(
        acquisition.clone().apply_checksum(exec_results_checksum),
        Err(Error::InvalidAttemptToApplyChecksum { .. })
    );

    assert_matches!(
        acquisition
            .clone()
            .apply_checksum(ExecutionResultsChecksum::Uncheckable),
        Err(Error::InvalidAttemptToApplyChecksum { .. })
    );

    let mut test_chunks = chunks_with_proof_from_data(&[0; ChunkWithProof::CHUNK_SIZE_BYTES]);
    assert_eq!(test_chunks.len(), 1);

    let exec_result = BlockExecutionResultsOrChunk::new_from_value(
        *block.hash(),
        ValueOrChunk::ChunkWithProof(test_chunks.remove(&0).unwrap()),
    );
    assert_matches!(
        acquisition.apply_block_execution_results_or_chunk(exec_result, vec![]),
        Err(Error::AttemptToApplyDataAfterCompleted { .. })
    );
}
