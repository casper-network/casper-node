use datasize::DataSize;

#[derive(Copy, Clone, PartialEq, Eq, DataSize, Debug)]
pub(crate) enum ReactorState {
    // get all components and reactor state set up on start
    Initialize,
    // orient to the network and attempt to catch up to tip
    CatchUp,
    // running commit upgrade and creating immediate switch block
    Upgrading,
    // stay caught up with tip
    KeepUp,
    // node is currently caught up and is an active validator
    Validate,
    // node should be shut down for upgrade
    ShutdownForUpgrade,
}
