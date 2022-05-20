//! Chainspec loader component.
//!
//! The chainspec loader initializes a node by reading information from the chainspec or an
//! upgrade_point, and committing it to the permanent storage.
//!
//! See
//! <https://casperlabs.atlassian.net/wiki/spaces/EN/pages/135528449/Genesis+Process+Specification>
//! for full details.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use std::{
    fmt::{self, Display, Formatter},
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use datasize::DataSize;
use derive_more::From;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio::task;
use tracing::{debug, error, info, trace, warn};

use casper_types::{file_utils, EraId, ProtocolVersion};

#[cfg(test)]
use crate::utils::RESOURCES_PATH;
use crate::{
    components::Component,
    effect::{
        announcements::ChainspecLoaderAnnouncement, requests::ChainspecLoaderRequest,
        EffectBuilder, EffectExt, Effects,
    },
    reactor::ReactorExit,
    storage::StorageRequest,
    types::{
        chainspec::{ChainspecRawBytes, Error, ProtocolConfig, CHAINSPEC_FILENAME},
        ActivationPoint, BlockHeader, Chainspec, ChainspecInfo, ExitCode,
    },
    utils::Loadable,
    NodeRng,
};

const UPGRADE_CHECK_INTERVAL: Duration = Duration::from_secs(60);

/// `ChainspecHandler` events.
#[derive(Debug, From, Serialize)]
pub(crate) enum Event {
    /// The result of getting the highest block from storage.
    Initialize {
        maybe_highest_block: Option<Box<BlockHeader>>,
    },
    #[from]
    Request(ChainspecLoaderRequest),
    /// Check config dir to see if an upgrade activation point is available, and if so announce it.
    CheckForNextUpgrade,
    /// If the result of checking for an upgrade is successful, it is passed here.
    GotNextUpgrade(NextUpgrade),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Initialize {
                maybe_highest_block,
            } => {
                write!(
                    formatter,
                    "initialize(maybe_highest_block: {})",
                    maybe_highest_block
                        .as_ref()
                        .map_or_else(|| "None".to_string(), |block| block.to_string())
                )
            }
            Event::Request(req) => write!(formatter, "chainspec_loader request: {}", req),
            Event::CheckForNextUpgrade => {
                write!(formatter, "check for next upgrade")
            }
            Event::GotNextUpgrade(next_upgrade) => {
                write!(formatter, "got {}", next_upgrade)
            }
        }
    }
}

/// Information about the next protocol upgrade.
#[derive(PartialEq, Eq, DataSize, Debug, Serialize, Deserialize, Clone, JsonSchema)]
pub struct NextUpgrade {
    activation_point: ActivationPoint,
    #[data_size(skip)]
    #[schemars(with = "String")]
    protocol_version: ProtocolVersion,
}

impl NextUpgrade {
    pub(crate) fn new(
        activation_point: ActivationPoint,
        protocol_version: ProtocolVersion,
    ) -> Self {
        NextUpgrade {
            activation_point,
            protocol_version,
        }
    }

    pub(crate) fn activation_point(&self) -> ActivationPoint {
        self.activation_point
    }
}

impl From<ProtocolConfig> for NextUpgrade {
    fn from(protocol_config: ProtocolConfig) -> Self {
        NextUpgrade {
            activation_point: protocol_config.activation_point,
            protocol_version: protocol_config.version,
        }
    }
}

impl Display for NextUpgrade {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "next upgrade to {} at start of era {}",
            self.protocol_version,
            self.activation_point.era_id()
        )
    }
}

/// Basic information about the current run of the node software.
#[derive(Clone, Debug)]
pub(crate) struct CurrentRunInfo {
    pub(crate) last_emergency_restart: Option<EraId>,
}

#[derive(Clone, DataSize, Debug)]
pub(crate) struct ChainspecLoader {
    chainspec: Arc<Chainspec>,
    chainspec_raw_bytes: Arc<ChainspecRawBytes>,
    /// The path to the folder where all chainspec and upgrade_point files will be stored in
    /// subdirs corresponding to their versions.
    root_dir: PathBuf,
    reactor_exit: Option<ReactorExit>,
    next_upgrade: Option<NextUpgrade>,
}

impl ChainspecLoader {
    pub(crate) fn new<P, REv>(
        chainspec_dir: P,
        effect_builder: EffectBuilder<REv>,
    ) -> Result<(Self, Effects<Event>), Error>
    where
        P: AsRef<Path>,
        REv: From<Event> + From<StorageRequest> + Send,
    {
        let (chainspec, chainspec_raw_bytes) =
            <(Chainspec, ChainspecRawBytes)>::from_path(&chainspec_dir.as_ref())?;
        Ok(Self::new_with_chainspec_and_path(
            Arc::new(chainspec),
            Arc::new(chainspec_raw_bytes),
            chainspec_dir,
            effect_builder,
        ))
    }

    #[cfg(test)]
    pub(crate) fn new_with_chainspec<REv>(
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        effect_builder: EffectBuilder<REv>,
    ) -> (Self, Effects<Event>)
    where
        REv: From<Event> + From<StorageRequest> + Send,
    {
        Self::new_with_chainspec_and_path(
            chainspec,
            chainspec_raw_bytes,
            &RESOURCES_PATH.join("local"),
            effect_builder,
        )
    }

    fn new_with_chainspec_and_path<P, REv>(
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        chainspec_dir: P,
        effect_builder: EffectBuilder<REv>,
    ) -> (Self, Effects<Event>)
    where
        P: AsRef<Path>,
        REv: From<Event> + From<StorageRequest> + Send,
    {
        let root_dir = chainspec_dir
            .as_ref()
            .parent()
            .map(|path| path.to_path_buf())
            .unwrap_or_else(|| {
                error!("chainspec dir must have a parent");
                PathBuf::new()
            });

        if !chainspec.is_valid() || root_dir.as_os_str().is_empty() {
            let chainspec_loader = ChainspecLoader {
                chainspec,
                chainspec_raw_bytes,
                root_dir,
                reactor_exit: Some(ReactorExit::ProcessShouldExit(ExitCode::Abort)),
                next_upgrade: None,
            };
            return (chainspec_loader, Effects::new());
        }

        let next_upgrade = next_upgrade(root_dir.clone(), chainspec.protocol_config.version);

        // If the next activation point is the same as the current chainspec one, we've installed
        // two new versions, where the first which we're currently running should be immediately
        // replaced by the second.
        let should_stop = if let Some(next_activation_point) = next_upgrade
            .as_ref()
            .map(|upgrade| upgrade.activation_point)
        {
            chainspec.protocol_config.activation_point == next_activation_point
        } else {
            false
        };

        let mut effects = if should_stop {
            Effects::new()
        } else {
            effect_builder
                .get_highest_block_header_from_storage()
                .event(|highest_block| Event::Initialize {
                    maybe_highest_block: highest_block.map(Box::new),
                })
        };

        // Start regularly checking for the next upgrade.
        effects.extend(
            effect_builder
                .set_timeout(UPGRADE_CHECK_INTERVAL)
                .event(|_| Event::CheckForNextUpgrade),
        );

        let reactor_exit = should_stop.then(|| ReactorExit::ProcessShouldExit(ExitCode::Success));

        let chainspec_loader = ChainspecLoader {
            chainspec,
            chainspec_raw_bytes,
            root_dir,
            reactor_exit,
            next_upgrade,
        };

        (chainspec_loader, effects)
    }

    fn handle_initialize<REv>(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        maybe_highest_block: Option<Box<BlockHeader>>,
    ) -> Effects<Event>
    where
        REv: Send,
    {
        self.reactor_exit = Some(
            Self::should_exit_for_upgrade(
                maybe_highest_block,
                self.next_upgrade_activation_point(),
            )
            .then(|| ReactorExit::ProcessShouldExit(ExitCode::Success))
            .unwrap_or(ReactorExit::ProcessShouldContinue),
        );
        Effects::new()
    }

    fn should_exit_for_upgrade(
        maybe_highest_block_header: Option<Box<BlockHeader>>,
        maybe_next_upgrade_activation_point: Option<ActivationPoint>,
    ) -> bool {
        maybe_highest_block_header.map_or(false, |highest_block_header| {
            maybe_next_upgrade_activation_point.map_or(false, |next_upgrade_activation_point| {
                if highest_block_header.next_block_era_id()
                    >= next_upgrade_activation_point.era_id()
                {
                    // This is an invalid run as the highest block era ID >= next activation
                    // point, so we're running an outdated version.  Exit with success to
                    // indicate we should upgrade.
                    warn!(
                        %next_upgrade_activation_point,
                        %highest_block_header,
                        "running outdated version: exit to upgrade"
                    );
                    true
                } else {
                    false
                }
            })
        })
    }

    /// This is a workaround while we have multiple reactors.  It should be used in the joiner and
    /// participating reactors' constructors to start the recurring task of checking for upgrades.
    /// The recurring tasks of the previous reactors will be cancelled when the relevant reactor
    /// is destroyed during transition.
    pub(crate) fn start_checking_for_upgrades<REv>(
        &self,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<ChainspecLoaderAnnouncement> + Send,
    {
        self.check_for_next_upgrade(effect_builder)
    }

    pub(crate) fn reactor_exit(&self) -> Option<ReactorExit> {
        self.reactor_exit
    }

    pub(crate) fn chainspec(&self) -> &Arc<Chainspec> {
        &self.chainspec
    }

    pub(crate) fn next_upgrade_activation_point(&self) -> Option<ActivationPoint> {
        self.next_upgrade
            .as_ref()
            .map(|next_upgrade| next_upgrade.activation_point())
    }

    /// Returns the era ID of where we should reset back to.  This means stored blocks in that and
    /// subsequent eras are deleted from storage.
    pub(crate) fn hard_reset_to_start_of_era(&self) -> Option<EraId> {
        self.chainspec
            .protocol_config
            .hard_reset
            .then(|| self.chainspec.protocol_config.activation_point.era_id())
    }

    fn new_chainspec_info(&self) -> ChainspecInfo {
        ChainspecInfo::new(
            self.chainspec.network_config.name.clone(),
            self.next_upgrade.clone(),
        )
    }

    fn get_current_run_info(&self) -> CurrentRunInfo {
        CurrentRunInfo {
            last_emergency_restart: self.chainspec.protocol_config.last_emergency_restart,
        }
    }

    fn check_for_next_upgrade<REv>(&self, effect_builder: EffectBuilder<REv>) -> Effects<Event>
    where
        REv: From<ChainspecLoaderAnnouncement> + Send,
    {
        let root_dir = self.root_dir.clone();
        let current_version = self.chainspec.protocol_config.version;
        let mut effects = async move {
            let maybe_next_upgrade =
                task::spawn_blocking(move || next_upgrade(root_dir, current_version))
                    .await
                    .unwrap_or_else(|error| {
                        warn!(%error, "failed to join tokio task");
                        None
                    });
            if let Some(next_upgrade) = maybe_next_upgrade {
                effect_builder
                    .announce_upgrade_activation_point_read(next_upgrade)
                    .await
            }
        }
        .ignore();

        effects.extend(
            effect_builder
                .set_timeout(UPGRADE_CHECK_INTERVAL)
                .event(|_| Event::CheckForNextUpgrade),
        );

        effects
    }

    fn handle_got_next_upgrade(&mut self, next_upgrade: NextUpgrade) -> Effects<Event> {
        debug!("got {}", next_upgrade);
        if let Some(ref current_point) = self.next_upgrade {
            if next_upgrade != *current_point {
                info!(
                    new_point=%next_upgrade.activation_point,
                    %current_point,
                    "changing upgrade activation point"
                );
            }
        }
        self.next_upgrade = Some(next_upgrade);
        Effects::new()
    }
}

impl<REv> Component<REv> for ChainspecLoader
where
    REv: From<Event> + From<ChainspecLoaderAnnouncement> + Send,
{
    type Event = Event;
    type ConstructionError = Error;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        trace!("{}", event);
        match event {
            Event::Initialize {
                maybe_highest_block,
            } => self.handle_initialize(effect_builder, maybe_highest_block),
            Event::Request(ChainspecLoaderRequest::GetChainspecInfo(responder)) => {
                responder.respond(self.new_chainspec_info()).ignore()
            }
            Event::Request(ChainspecLoaderRequest::GetCurrentRunInfo(responder)) => {
                responder.respond(self.get_current_run_info()).ignore()
            }
            Event::Request(ChainspecLoaderRequest::GetChainspecRawBytes(responder)) => responder
                .respond(Arc::clone(&self.chainspec_raw_bytes))
                .ignore(),
            Event::CheckForNextUpgrade => self.check_for_next_upgrade(effect_builder),
            Event::GotNextUpgrade(next_upgrade) => self.handle_got_next_upgrade(next_upgrade),
        }
    }
}

/// This struct can be parsed from a TOML-encoded chainspec file.  It means that as the
/// chainspec format changes over versions, as long as we maintain the protocol config in this form
/// in the chainspec file, it can continue to be parsed as an `UpgradePoint`.
#[derive(Deserialize)]
struct UpgradePoint {
    #[serde(rename = "protocol")]
    pub(crate) protocol_config: ProtocolConfig,
}

impl UpgradePoint {
    /// Parses a chainspec file at the given path as an `UpgradePoint`.
    fn from_chainspec_path<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let bytes = file_utils::read_file(path.as_ref().join(&CHAINSPEC_FILENAME))
            .map_err(Error::LoadUpgradePoint)?;
        Ok(toml::from_slice(&bytes)?)
    }
}

fn dir_name_from_version(version: &ProtocolVersion) -> PathBuf {
    PathBuf::from(version.to_string().replace('.', "_"))
}

/// Iterates the given path, returning the subdir representing the immediate next SemVer version
/// after `current_version`.
///
/// Subdir names should be semvers with dots replaced with underscores.
fn next_installed_version(
    dir: &Path,
    current_version: &ProtocolVersion,
) -> Result<ProtocolVersion, Error> {
    let max_version =
        ProtocolVersion::from_parts(u32::max_value(), u32::max_value(), u32::max_value());

    let mut next_version = max_version;
    let mut read_version = false;
    for entry in fs::read_dir(dir).map_err(|error| Error::ReadDir {
        dir: dir.to_path_buf(),
        error,
    })? {
        let path = match entry {
            Ok(dir_entry) => dir_entry.path(),
            Err(error) => {
                debug!(dir=%dir.display(), %error, "bad entry while reading dir");
                continue;
            }
        };

        let subdir_name = match path.file_name() {
            Some(name) => name.to_string_lossy().replace('_', "."),
            None => continue,
        };

        let version = match ProtocolVersion::from_str(&subdir_name) {
            Ok(version) => version,
            Err(error) => {
                trace!(%error, path=%path.display(), "failed to get a version");
                continue;
            }
        };

        if version > *current_version && version < next_version {
            next_version = version;
        }
        read_version = true;
    }

    if !read_version {
        return Err(Error::NoVersionSubdirFound {
            dir: dir.to_path_buf(),
        });
    }

    if next_version == max_version {
        next_version = *current_version;
    }

    Ok(next_version)
}

/// Uses `next_installed_version()` to find the next versioned subdir.  If it exists, reads the
/// UpgradePoint file from there and returns its version and activation point.  Returns `None` if
/// there is no greater version available, or if any step errors.
fn next_upgrade(dir: PathBuf, current_version: ProtocolVersion) -> Option<NextUpgrade> {
    let next_version = match next_installed_version(&dir, &current_version) {
        Ok(version) => version,
        Err(error) => {
            warn!(dir=%dir.display(), %error, "failed to get a valid version from subdirs");
            return None;
        }
    };

    if next_version <= current_version {
        return None;
    }

    let subdir = dir.join(dir_name_from_version(&next_version));
    let upgrade_point = match UpgradePoint::from_chainspec_path(&subdir) {
        Ok(upgrade_point) => upgrade_point,
        Err(error) => {
            debug!(subdir=%subdir.display(), %error, "failed to load upgrade point");
            return None;
        }
    };

    if upgrade_point.protocol_config.version != next_version {
        warn!(
            upgrade_point_version=%upgrade_point.protocol_config.version,
            subdir_version=%next_version,
            "next chainspec installed to wrong subdir"
        );
        return None;
    }

    Some(NextUpgrade::from(upgrade_point.protocol_config))
}

#[cfg(test)]
mod tests {
    use casper_types::testing::TestRng;

    use super::*;
    use crate::types::{chainspec::CHAINSPEC_FILENAME, Block};

    #[test]
    fn correctly_detects_when_to_exit_for_upgrade() {
        let mut rng = crate::new_rng();
        const HEIGHT: u64 = 10;
        const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion::V1_0_0;
        const IS_NOT_SWITCH: bool = false;
        const IS_SWITCH: bool = true;
        const VERIFIABLE_CHUNKED_HASH_ACTIVATION: u64 = 10;

        let highest_block_header = None;
        let next_upgrade_activation_point = None;
        assert!(!ChainspecLoader::should_exit_for_upgrade(
            highest_block_header,
            next_upgrade_activation_point
        ));

        let highest_block_header = Some(Box::new(
            Block::random_with_specifics(
                &mut rng,
                EraId::from(2),
                HEIGHT,
                PROTOCOL_VERSION,
                IS_NOT_SWITCH,
                VERIFIABLE_CHUNKED_HASH_ACTIVATION.into(),
                None,
            )
            .header()
            .clone(),
        ));
        let next_upgrade_activation_point = None;
        assert!(!ChainspecLoader::should_exit_for_upgrade(
            highest_block_header,
            next_upgrade_activation_point
        ));

        let highest_block_header = None;
        let next_upgrade_activation_point = Some(ActivationPoint::EraId(10.into()));
        assert!(!ChainspecLoader::should_exit_for_upgrade(
            highest_block_header,
            next_upgrade_activation_point
        ));

        let highest_block_header = Some(Box::new(
            Block::random_with_specifics(
                &mut rng,
                EraId::from(2),
                HEIGHT,
                PROTOCOL_VERSION,
                IS_NOT_SWITCH,
                VERIFIABLE_CHUNKED_HASH_ACTIVATION.into(),
                None,
            )
            .header()
            .clone(),
        ));
        let next_upgrade_activation_point = Some(ActivationPoint::EraId(3.into()));
        assert!(!ChainspecLoader::should_exit_for_upgrade(
            highest_block_header,
            next_upgrade_activation_point
        ));

        let highest_block_header = Some(Box::new(
            Block::random_with_specifics(
                &mut rng,
                EraId::from(2),
                HEIGHT,
                PROTOCOL_VERSION,
                IS_NOT_SWITCH,
                VERIFIABLE_CHUNKED_HASH_ACTIVATION.into(),
                None,
            )
            .header()
            .clone(),
        ));
        let next_upgrade_activation_point = Some(ActivationPoint::EraId(2.into()));
        assert!(ChainspecLoader::should_exit_for_upgrade(
            highest_block_header,
            next_upgrade_activation_point
        ));

        let highest_block_header = Some(Box::new(
            Block::random_with_specifics(
                &mut rng,
                EraId::from(2),
                HEIGHT,
                PROTOCOL_VERSION,
                IS_SWITCH,
                VERIFIABLE_CHUNKED_HASH_ACTIVATION.into(),
                None,
            )
            .header()
            .clone(),
        ));
        let next_upgrade_activation_point = Some(ActivationPoint::EraId(3.into()));
        assert!(ChainspecLoader::should_exit_for_upgrade(
            highest_block_header,
            next_upgrade_activation_point
        ));
    }

    #[test]
    fn should_get_next_installed_version() {
        let tempdir = tempfile::tempdir().expect("should create temp dir");

        let get_next_version = |current_version: &ProtocolVersion| {
            next_installed_version(tempdir.path(), current_version).unwrap()
        };

        let mut current = ProtocolVersion::from_parts(0, 0, 0);
        let mut next_version = ProtocolVersion::from_parts(1, 0, 0);
        fs::create_dir(tempdir.path().join("1_0_0")).unwrap();
        assert_eq!(get_next_version(&current), next_version);
        current = next_version;

        next_version = ProtocolVersion::from_parts(1, 2, 3);
        fs::create_dir(tempdir.path().join("1_2_3")).unwrap();
        assert_eq!(get_next_version(&current), next_version);
        current = next_version;

        fs::create_dir(tempdir.path().join("1_0_3")).unwrap();
        assert_eq!(get_next_version(&current), next_version);

        fs::create_dir(tempdir.path().join("2_2_2")).unwrap();
        fs::create_dir(tempdir.path().join("3_3_3")).unwrap();
        assert_eq!(
            get_next_version(&current),
            ProtocolVersion::from_parts(2, 2, 2)
        );
    }

    #[test]
    fn should_ignore_invalid_versions() {
        let tempdir = tempfile::tempdir().expect("should create temp dir");

        // Executes `next_installed_version()` and asserts the resulting error as a string starts
        // with the given text.
        let min_version = ProtocolVersion::from_parts(0, 0, 0);
        let assert_error_starts_with = |path: &Path, expected: String| {
            let error_msg = next_installed_version(path, &min_version)
                .unwrap_err()
                .to_string();
            assert!(
                error_msg.starts_with(&expected),
                "Error message expected to start with \"{}\"\nActual error message: \"{}\"",
                expected,
                error_msg
            );
        };

        // Try with a non-existent dir.
        let non_existent_dir = Path::new("not_a_dir");
        assert_error_starts_with(
            non_existent_dir,
            format!("failed to read dir {}", non_existent_dir.display()),
        );

        // Try with a dir which has no subdirs.
        assert_error_starts_with(
            tempdir.path(),
            format!(
                "failed to get a valid version from subdirs in {}",
                tempdir.path().display()
            ),
        );

        // Try with a dir which has one subdir which is not a valid version representation.
        fs::create_dir(tempdir.path().join("not_a_version")).unwrap();
        assert_error_starts_with(
            tempdir.path(),
            format!(
                "failed to get a valid version from subdirs in {}",
                tempdir.path().display()
            ),
        );

        // Try with a dir which has a valid and invalid subdir - the invalid one should be ignored.
        fs::create_dir(tempdir.path().join("1_2_3")).unwrap();
        assert_eq!(
            next_installed_version(tempdir.path(), &min_version).unwrap(),
            ProtocolVersion::from_parts(1, 2, 3)
        );
    }

    /// Creates the appropriate subdir in `root_dir`, and adds a random chainspec.toml with the
    /// protocol_config.version field set to `version`.
    fn install_chainspec(
        rng: &mut TestRng,
        root_dir: &Path,
        version: &ProtocolVersion,
    ) -> Chainspec {
        let mut chainspec = Chainspec::random(rng);
        chainspec.protocol_config.version = *version;

        let subdir = root_dir.join(dir_name_from_version(version));
        fs::create_dir(&subdir).unwrap();

        let path = subdir.join(CHAINSPEC_FILENAME);
        fs::write(
            path,
            toml::to_string_pretty(&chainspec).expect("should encode to toml"),
        )
        .expect("should install chainspec");
        chainspec
    }

    #[test]
    fn should_get_next_upgrade() {
        let tempdir = tempfile::tempdir().expect("should create temp dir");

        let next_point = |current_version: &ProtocolVersion| {
            next_upgrade(tempdir.path().to_path_buf(), *current_version).unwrap()
        };

        let mut rng = crate::new_rng();

        let mut current = ProtocolVersion::from_parts(0, 9, 9);
        let v1_0_0 = ProtocolVersion::from_parts(1, 0, 0);
        let chainspec_v1_0_0 = install_chainspec(&mut rng, tempdir.path(), &v1_0_0);
        assert_eq!(
            next_point(&current),
            chainspec_v1_0_0.protocol_config.into()
        );

        current = v1_0_0;
        let v1_0_3 = ProtocolVersion::from_parts(1, 0, 3);
        let chainspec_v1_0_3 = install_chainspec(&mut rng, tempdir.path(), &v1_0_3);
        assert_eq!(
            next_point(&current),
            chainspec_v1_0_3.protocol_config.into()
        );
    }

    #[test]
    fn should_not_get_old_or_invalid_upgrade() {
        let tempdir = tempfile::tempdir().expect("should create temp dir");

        let maybe_next_point = |current_version: &ProtocolVersion| {
            next_upgrade(tempdir.path().to_path_buf(), *current_version)
        };

        let mut rng = crate::new_rng();

        // Check we return `None` if there are no version subdirs.
        let v1_0_0 = ProtocolVersion::from_parts(1, 0, 0);
        let mut current = v1_0_0;
        assert!(maybe_next_point(&current).is_none());

        // Check we return `None` if current_version == next_version.
        let chainspec_v1_0_0 = install_chainspec(&mut rng, tempdir.path(), &v1_0_0);
        assert!(maybe_next_point(&current).is_none());

        // Check we return `None` if current_version > next_version.
        current = ProtocolVersion::from_parts(2, 0, 0);
        assert!(maybe_next_point(&current).is_none());

        // Check we return `None` if we find an upgrade file where the protocol_config.version field
        // doesn't match the subdir name.
        let v0_9_9 = ProtocolVersion::from_parts(0, 9, 9);
        current = v0_9_9;
        assert!(maybe_next_point(&current).is_some());

        let mut chainspec_v0_9_9 = chainspec_v1_0_0;
        chainspec_v0_9_9.protocol_config.version = v0_9_9;
        let path_v1_0_0 = tempdir
            .path()
            .join(dir_name_from_version(&v1_0_0))
            .join(CHAINSPEC_FILENAME);
        fs::write(
            &path_v1_0_0,
            toml::to_string_pretty(&chainspec_v0_9_9).expect("should encode to toml"),
        )
        .expect("should install upgrade point");
        assert!(maybe_next_point(&current).is_none());

        // Check we return `None` if the next version upgrade_point file is corrupt.
        fs::write(&path_v1_0_0, "bad data".as_bytes()).unwrap();
        assert!(maybe_next_point(&current).is_none());

        // Check we return `None` if the next version upgrade_point file is missing.
        fs::remove_file(&path_v1_0_0).unwrap();
        assert!(maybe_next_point(&current).is_none());
    }
}
