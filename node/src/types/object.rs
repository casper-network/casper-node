use serde::{Deserialize, Serialize};

use super::Deploy;

/// A Casper object, capable of being sent across the network.
#[derive(Clone, Debug, Deserialize, Serialize)]
enum Object {
    /// A deploy.
    Deploy(Deploy),
}
