use thiserror::Error;

use crate::{reactor::validator::Config, utils::WithDir};

// This will be changed in favour of an actual old config type when the migration is not a no-op.
type OldConfig = Config;

/// Error returned as a result of migrating the config file.
#[derive(Debug, Error)]
pub enum Error {}

/// Migrates values from the old config file to the new one, modifying the new config file on-disk.
///
/// This should be executed after a new version is available, but before the casper-node has been
/// run in validator mode using the new version.
pub fn migrate_config(
    _old_config: WithDir<OldConfig>,
    _new_config: WithDir<Config>,
) -> Result<(), Error> {
    Ok(())
}
