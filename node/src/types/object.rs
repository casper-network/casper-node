use serde::{Deserialize, Serialize};

/// A Casper object, capable of being sent across the network.
#[derive(Clone, Debug, Deserialize, Serialize)]
enum Object {}
