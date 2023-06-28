mod block_body_v1;
mod block_body_v2;
mod versioned_block_body;

pub use block_body_v1::BlockBodyV1;
pub use block_body_v2::BlockBodyV2;
pub use versioned_block_body::VersionedBlockBody;

/// The body portion of a block.
pub type BlockBody = BlockBodyV2;
