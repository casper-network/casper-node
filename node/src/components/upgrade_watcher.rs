//! Chainspec loader component.
//!
//! The chainspec loader initializes a node by reading information from the chainspec or an
//! upgrade_point, and committing it to the permanent storage.
//!
//! See
//! <https://casperlabs.atlassian.net/wiki/spaces/EN/pages/135528449/Genesis+Process+Specification>
//! for full details.

use std::{
    fmt::{self, Display, Formatter},
    fs, io,
    path::{Path, PathBuf},
    str::FromStr,
};

use datasize::DataSize;
use derive_more::From;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::task;
use tracing::{debug, error, info, trace, warn};

use casper_types::{
    file_utils::{self, ReadFileError},
    EraId, ProtocolVersion, TimeDiff,
};

use crate::{
    components::{Component, ComponentState, InitializedComponent},
    effect::{
        announcements::UpgradeWatcherAnnouncement, requests::UpgradeWatcherRequest, EffectBuilder,
        EffectExt, Effects,
    },
    reactor::main_reactor::MainEvent,
    types::{
        chainspec::{ProtocolConfig, CHAINSPEC_FILENAME},
        ActivationPoint, Chainspec,
    },
    NodeRng,
};

const COMPONENT_NAME: &str = "upgrade_watcher";

const DEFAULT_UPGRADE_CHECK_INTERVAL: &str = "30sec";

#[derive(Copy, Clone, DataSize, Debug, Deserialize, Serialize)]
pub struct Config {
    /// How often to scan file system for available upgrades.
    pub upgrade_check_interval: TimeDiff,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            upgrade_check_interval: DEFAULT_UPGRADE_CHECK_INTERVAL.parse().unwrap(),
        }
    }
}

/// `ChainspecHandler` events.
#[derive(Debug, From, Serialize)]
pub(crate) enum Event {
    /// Start checking for installed upgrades.
    Initialize,
    #[from]
    Request(UpgradeWatcherRequest),
    /// Check config dir to see if an upgrade activation point is available, and if so announce it.
    CheckForNextUpgrade,
    /// If the result of checking for an upgrade is successful, it is passed here.
    GotNextUpgrade(Option<NextUpgrade>),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Initialize => {
                write!(formatter, "start checking for installed upgrades")
            }
            Event::Request(_) => {
                write!(formatter, "upgrade watcher request")
            }
            Event::CheckForNextUpgrade => {
                write!(formatter, "check for next upgrade")
            }
            Event::GotNextUpgrade(Some(next_upgrade)) => {
                write!(formatter, "got {}", next_upgrade)
            }
            Event::GotNextUpgrade(None) => {
                write!(formatter, "no upgrade detected")
            }
        }
    }
}

#[derive(Debug, Error)]
pub(crate) enum Error {
    /// Error while decoding the chainspec from TOML format.
    #[error("decoding from TOML error: {0}")]
    DecodingFromToml(#[from] toml::de::Error),

    #[error("chainspec directory does not have a parent")]
    NoChainspecDirParent,

    /// Error loading the upgrade point.
    #[error("could not load upgrade point: {0}")]
    LoadUpgradePoint(ReadFileError),

    /// Failed to read the given directory.
    #[error("failed to read dir {}: {error}", dir.display())]
    ReadDir {
        /// The directory which could not be read.
        dir: PathBuf,
        /// The underlying error.
        error: io::Error,
    },

    /// No subdirectory representing a semver version was found in the given directory.
    #[error("failed to get a valid version from subdirs in {}", dir.display())]
    NoVersionSubdirFound {
        /// The searched directory.
        dir: PathBuf,
    },
}

/// Information about the next protocol upgrade.
#[derive(PartialEq, Eq, DataSize, Debug, Serialize, Deserialize, Clone, Copy, JsonSchema)]
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

#[derive(Clone, DataSize, Debug)]
pub(crate) struct UpgradeWatcher {
    current_version: ProtocolVersion,
    config: Config,
    /// The path to the folder where all chainspec and upgrade_point files will be stored in
    /// subdirs corresponding to their versions.
    root_dir: PathBuf,
    state: ComponentState,
    next_upgrade: Option<NextUpgrade>,
}

impl UpgradeWatcher {
    pub(crate) fn new<P: AsRef<Path>>(
        chainspec: &Chainspec,
        config: Config,
        chainspec_dir: P,
    ) -> Result<Self, Error> {
        let root_dir = chainspec_dir
            .as_ref()
            .parent()
            .map(|path| path.to_path_buf())
            .ok_or(Error::NoChainspecDirParent)?;

        let current_version = chainspec.protocol_config.version;
        let next_upgrade = next_upgrade(root_dir.clone(), current_version);

        let upgrade_watcher = UpgradeWatcher {
            current_version,
            config,
            root_dir,
            state: ComponentState::Uninitialized,
            next_upgrade,
        };

        Ok(upgrade_watcher)
    }

    pub(crate) fn should_upgrade_after(&self, era_id: EraId) -> bool {
        self.next_upgrade.as_ref().map_or(false, |upgrade| {
            upgrade.activation_point.should_upgrade(&era_id)
        })
    }

    pub(crate) fn next_upgrade_activation_point(&self) -> Option<EraId> {
        self.next_upgrade
            .map(|next_upgrade| next_upgrade.activation_point.era_id())
    }

    fn start_checking_for_upgrades<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
    ) -> Effects<Event>
    where
        REv: From<UpgradeWatcherAnnouncement> + Send,
    {
        if self.state != ComponentState::Initializing {
            return Effects::new();
        }
        <Self as InitializedComponent<MainEvent>>::set_state(self, ComponentState::Initialized);
        self.check_for_next_upgrade(effect_builder)
    }

    fn check_for_next_upgrade<REv>(&self, effect_builder: EffectBuilder<REv>) -> Effects<Event>
    where
        REv: From<UpgradeWatcherAnnouncement> + Send,
    {
        let root_dir = self.root_dir.clone();
        let current_version = self.current_version;
        let mut effects = async move {
            let maybe_next_upgrade =
                task::spawn_blocking(move || next_upgrade(root_dir, current_version))
                    .await
                    .unwrap_or_else(|error| {
                        warn!(%error, "failed to join tokio task");
                        None
                    });
            effect_builder
                .upgrade_watcher_announcement(maybe_next_upgrade)
                .await
        }
        .ignore();

        effects.extend(
            effect_builder
                .set_timeout(self.config.upgrade_check_interval.into())
                .event(|_| Event::CheckForNextUpgrade),
        );

        effects
    }

    fn handle_got_next_upgrade(
        &mut self,
        maybe_next_upgrade: Option<NextUpgrade>,
    ) -> Effects<Event> {
        trace!("got {:?}", maybe_next_upgrade);
        if self.next_upgrade != maybe_next_upgrade {
            let new_point = match &maybe_next_upgrade {
                Some(next_upgrade) => next_upgrade.to_string(),
                None => "none".to_string(),
            };
            let current_point = match &self.next_upgrade {
                Some(next_upgrade) => next_upgrade.to_string(),
                None => "none".to_string(),
            };
            info!(
                %new_point,
                %current_point,
                "changing upgrade activation point"
            );
        }

        self.next_upgrade = maybe_next_upgrade;
        Effects::new()
    }
}

impl<REv> Component<REv> for UpgradeWatcher
where
    REv: From<Event> + From<UpgradeWatcherAnnouncement> + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match &self.state {
            ComponentState::Fatal(msg) => {
                error!(
                    msg,
                    ?event,
                    name = <Self as Component<MainEvent>>::name(self),
                    "should not handle this event when this component has fatal error"
                );
                Effects::new()
            }
            ComponentState::Uninitialized => {
                warn!(
                    ?event,
                    name = <Self as Component<MainEvent>>::name(self),
                    "should not handle this event when component is uninitialized"
                );
                Effects::new()
            }
            ComponentState::Initializing => match event {
                Event::Initialize => self.start_checking_for_upgrades(effect_builder),
                Event::Request(_) | Event::CheckForNextUpgrade | Event::GotNextUpgrade(_) => {
                    warn!(
                        ?event,
                        name = <Self as Component<MainEvent>>::name(self),
                        "should not handle this event when component is pending initialization"
                    );
                    Effects::new()
                }
            },
            ComponentState::Initialized => match event {
                Event::Initialize => {
                    error!(
                        ?event,
                        name = <Self as Component<MainEvent>>::name(self),
                        "component already initialized"
                    );
                    Effects::new()
                }
                Event::Request(request) => request.0.respond(self.next_upgrade).ignore(),
                Event::CheckForNextUpgrade => self.check_for_next_upgrade(effect_builder),
                Event::GotNextUpgrade(next_upgrade) => self.handle_got_next_upgrade(next_upgrade),
            },
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

impl<REv> InitializedComponent<REv> for UpgradeWatcher
where
    REv: From<Event> + From<UpgradeWatcherAnnouncement> + Send,
{
    fn state(&self) -> &ComponentState {
        &self.state
    }

    fn set_state(&mut self, new_state: ComponentState) {
        info!(
            ?new_state,
            name = <Self as Component<MainEvent>>::name(self),
            "component state changed"
        );

        self.state = new_state;
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
        let bytes = file_utils::read_file(path.as_ref().join(CHAINSPEC_FILENAME))
            .map_err(Error::LoadUpgradePoint)?;
        Ok(toml::from_slice(&bytes)?)
    }
}

fn dir_name_from_version(version: ProtocolVersion) -> PathBuf {
    PathBuf::from(version.to_string().replace('.', "_"))
}

/// Iterates the given path, returning the subdir representing the immediate next SemVer version
/// after `current_version`.  If no higher version than `current_version` is found, then
/// `current_version` is returned.
///
/// Subdir names should be semvers with dots replaced with underscores.
fn next_installed_version(
    dir: &Path,
    current_version: ProtocolVersion,
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
                trace!(%error, path=%path.display(), "UpgradeWatcher: failed to get a version");
                continue;
            }
        };

        if version > current_version && version < next_version {
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
        next_version = current_version;
    }

    Ok(next_version)
}

/// Uses `next_installed_version()` to find the next versioned subdir.  If it exists, reads the
/// UpgradePoint file from there and returns its version and activation point.  Returns `None` if
/// there is no greater version available, or if any step errors.
fn next_upgrade(dir: PathBuf, current_version: ProtocolVersion) -> Option<NextUpgrade> {
    let next_version = match next_installed_version(&dir, current_version) {
        Ok(version) => version,
        Err(_error) => {
            #[cfg(not(test))]
            warn!(dir=%dir.display(), error=%_error, "failed to get a valid version from subdirs");
            return None;
        }
    };

    if next_version <= current_version {
        return None;
    }

    let subdir = dir.join(dir_name_from_version(next_version));
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
    use crate::{
        logging,
        types::{
            chainspec::{ActivationPoint, CHAINSPEC_FILENAME},
            ChainspecRawBytes,
        },
        utils::Loadable,
    };

    const V0_0_0: ProtocolVersion = ProtocolVersion::from_parts(0, 0, 0);
    const V0_9_9: ProtocolVersion = ProtocolVersion::from_parts(0, 9, 9);
    const V1_0_0: ProtocolVersion = ProtocolVersion::from_parts(1, 0, 0);
    const V1_0_3: ProtocolVersion = ProtocolVersion::from_parts(1, 0, 3);
    const V1_2_3: ProtocolVersion = ProtocolVersion::from_parts(1, 2, 3);
    const V2_2_2: ProtocolVersion = ProtocolVersion::from_parts(2, 2, 2);

    #[test]
    fn should_get_next_installed_version() {
        let tempdir = tempfile::tempdir().expect("should create temp dir");

        let get_next_version = |current_version: ProtocolVersion| {
            next_installed_version(tempdir.path(), current_version).unwrap()
        };

        // Should get next version (major version bump).
        fs::create_dir(tempdir.path().join("1_0_0")).unwrap();
        assert_eq!(get_next_version(V0_0_0), V1_0_0);

        // Should get next version (minor version bump).
        fs::create_dir(tempdir.path().join("1_2_3")).unwrap();
        assert_eq!(get_next_version(V1_0_0), V1_2_3);

        // Should report current as next version if only lower versions staged.
        fs::create_dir(tempdir.path().join("1_0_3")).unwrap();
        assert_eq!(get_next_version(V1_2_3), V1_2_3);

        // Should report lower of two higher versions.
        fs::create_dir(tempdir.path().join("2_2_2")).unwrap();
        fs::create_dir(tempdir.path().join("3_3_3")).unwrap();
        assert_eq!(get_next_version(V1_2_3), V2_2_2);

        // If higher versions unstaged, should report current again.
        fs::remove_dir_all(tempdir.path().join("2_2_2")).unwrap();
        fs::remove_dir_all(tempdir.path().join("3_3_3")).unwrap();
        assert_eq!(get_next_version(V1_2_3), V1_2_3);
    }

    #[test]
    fn should_ignore_invalid_versions() {
        let tempdir = tempfile::tempdir().expect("should create temp dir");

        // Executes `next_installed_version()` and asserts the resulting error as a string starts
        // with the given text.
        let min_version = V0_0_0;
        let assert_error_starts_with = |path: &Path, expected: String| {
            let error_msg = next_installed_version(path, min_version)
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
            next_installed_version(tempdir.path(), min_version).unwrap(),
            V1_2_3
        );
    }

    /// Creates the appropriate subdir in `root_dir`, and adds a random chainspec.toml with the
    /// protocol_config.version field set to `version`.
    fn install_chainspec(
        rng: &mut TestRng,
        root_dir: &Path,
        version: ProtocolVersion,
    ) -> Chainspec {
        let mut chainspec = Chainspec::random(rng);
        chainspec.protocol_config.version = version;

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

        let next_point = |current_version: ProtocolVersion| {
            next_upgrade(tempdir.path().to_path_buf(), current_version).unwrap()
        };

        let mut rng = crate::new_rng();

        let chainspec_v1_0_0 = install_chainspec(&mut rng, tempdir.path(), V1_0_0);
        assert_eq!(next_point(V0_9_9), chainspec_v1_0_0.protocol_config.into());

        let chainspec_v1_0_3 = install_chainspec(&mut rng, tempdir.path(), V1_0_3);
        assert_eq!(next_point(V1_0_0), chainspec_v1_0_3.protocol_config.into());
    }

    #[test]
    fn should_not_get_old_or_invalid_upgrade() {
        let tempdir = tempfile::tempdir().expect("should create temp dir");

        let maybe_next_point = |current_version: ProtocolVersion| {
            next_upgrade(tempdir.path().to_path_buf(), current_version)
        };

        let mut rng = crate::new_rng();

        // Check we return `None` if there are no version subdirs.
        assert!(maybe_next_point(V1_0_0).is_none());

        // Check we return `None` if current_version == next_version.
        let chainspec_v1_0_0 = install_chainspec(&mut rng, tempdir.path(), V1_0_0);
        assert!(maybe_next_point(V1_0_0).is_none());

        // Check we return `None` if current_version > next_version.
        assert!(maybe_next_point(V2_2_2).is_none());

        // Check we return `None` if we find an upgrade file where the protocol_config.version field
        // doesn't match the subdir name.
        assert!(maybe_next_point(V0_9_9).is_some());

        let mut chainspec_v0_9_9 = chainspec_v1_0_0;
        chainspec_v0_9_9.protocol_config.version = V0_9_9;
        let path_v1_0_0 = tempdir
            .path()
            .join(dir_name_from_version(V1_0_0))
            .join(CHAINSPEC_FILENAME);
        fs::write(
            &path_v1_0_0,
            toml::to_string_pretty(&chainspec_v0_9_9).expect("should encode to toml"),
        )
        .expect("should install upgrade point");
        assert!(maybe_next_point(V0_9_9).is_none());

        // Check we return `None` if the next version upgrade_point file is corrupt.
        fs::write(&path_v1_0_0, "bad data".as_bytes()).unwrap();
        assert!(maybe_next_point(V0_9_9).is_none());

        // Check we return `None` if the next version upgrade_point file is missing.
        fs::remove_file(&path_v1_0_0).unwrap();
        assert!(maybe_next_point(V0_9_9).is_none());
    }

    #[test]
    fn should_register_unstaged_upgrade() {
        let _ = logging::init();
        let tempdir = tempfile::tempdir().expect("should create temp dir");
        let (chainspec, _) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");
        let mut upgrade_watcher =
            UpgradeWatcher::new(&chainspec, Config::default(), tempdir.path()).unwrap();
        assert!(upgrade_watcher.next_upgrade.is_none());

        let next_upgrade = NextUpgrade {
            activation_point: ActivationPoint::EraId(EraId::MAX),
            protocol_version: ProtocolVersion::from_parts(9, 9, 9),
        };
        let _ = upgrade_watcher.handle_got_next_upgrade(Some(next_upgrade));
        assert_eq!(Some(next_upgrade), upgrade_watcher.next_upgrade);

        let _ = upgrade_watcher.handle_got_next_upgrade(None);
        assert!(upgrade_watcher.next_upgrade.is_none());
    }
}
