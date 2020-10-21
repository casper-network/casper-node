use casper_types::{account::Weight, PublicKey};
use std::{collections::HashMap, time::Duration};

#[derive(Debug)]
struct Scenario {
    initial_validators: HashMap<PublicKey, Weight>,
    non_participating_validators: usize,
    command: Vec<Command>,
}

#[derive(Copy, Clone, Debug)]
enum Command {
    /// Start up a node.
    StartNode { index: usize },
    /// Stop a running node.
    StopNode { index: usize },
    /// Run for a fixed duration.
    RunDuration { duration: Duration },
    /// Run until `num_era_changes` have been observed.
    RunEras { num_era_changes: usize },
    /// Run until `num_blocks_created` blocks have been created.
    RunBlocks { num_blocks_created: usize },
}
