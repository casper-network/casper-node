// use crate::tracking_copy::TrackingCopyError;
// use casper_types::Digest;
//
// /// Request to run genesis.
// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
// pub struct GenesisRequest {
//     state_hash: Digest,
// }
//
// impl GenesisRequest {
//     /// Creates an instance of GenesisRequest.
//     pub fn new(state_hash: Digest) -> Self {
//         GenesisRequest { state_hash }
//     }
//
//     /// Returns state root hash.
//     pub fn state_hash(&self) -> Digest {
//         self.state_hash
//     }
// }
//
// pub enum GenesisResult {
//     Fatal(String),
//     Failure(TrackingCopyError),
// }
