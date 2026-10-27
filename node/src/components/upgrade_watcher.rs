//! Chainspec loader component.
//!
//! The chainspec loader initializes a node by reading information from the chainspec or an
//! upgrade_point, and committing it to the permanent storage.
//!
//! See
//! <https://casperlabs.atlassian.net/wiki/spaces/EN/pages/135528449/Genesis+Process+Specification>
//! for full details.

use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Display, Formatter},
    fs, io,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use datasize::DataSize;
use derive_more::From;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::task;
use tracing::{debug, error, info, trace, warn};

use casper_types::{
    bytesrepr::Bytes, evm, Chainspec, ChainspecRawBytes, EraId, NextUpgrade, ProtocolConfig,
    ProtocolVersion, TimeDiff,
};

use crate::{
    components::{Component, ComponentState, InitializedComponent},
    effect::{
        announcements::UpgradeWatcherAnnouncement, requests::UpgradeWatcherRequest, EffectBuilder,
        EffectExt, Effects,
    },
    reactor::main_reactor::MainEvent,
    utils::{
        chain_specification::{
            parse_toml::{
                CHAINSPEC_ACCOUNTS_FILENAME, CHAINSPEC_FILENAME, CHAINSPEC_GLOBAL_STATE_FILENAME,
            },
            validate_evm_preinstalls,
        },
        evm_preinstalls::load_preinstall_bytecodes,
    },
    NodeRng,
};

const COMPONENT_NAME: &str = "upgrade_watcher";

const DEFAULT_UPGRADE_CHECK_INTERVAL: &str = "30sec";

type StagedChainspecs = BTreeMap<ProtocolVersion, Arc<ChainspecRawBytes>>;

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
    /// Atomically refresh complete chainspec documents, even when activation points are unchanged.
    GotStagedChainspecs {
        scan_id: u64,
        metadata: Box<Result<StagedChainspecs, String>>,
    },
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
            Event::GotStagedChainspecs { .. } => {
                write!(formatter, "refresh chainspec source documents")
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
    LoadUpgradePoint(io::Error),

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

#[derive(Clone, DataSize, Debug)]
pub(crate) struct UpgradeWatcher {
    current_version: ProtocolVersion,
    config: Config,
    /// The path to the folder where all chainspec and upgrade_point files will be stored in
    /// subdirs corresponding to their versions.
    root_dir: PathBuf,
    state: ComponentState,
    next_upgrade: Option<NextUpgrade>,
    network_name: String,
    metadata_scan_id: u64,
    #[data_size(skip)]
    chainspecs: Result<StagedChainspecs, String>,
    announced_protocols: BTreeSet<ProtocolVersion>,
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

        let network_name = chainspec.network_config.name.clone();
        let chainspecs = installed_chainspecs(&root_dir, &network_name, current_version);
        if let Err(error) = &chainspecs {
            warn!(%error, "invalid installed chainspec metadata");
        }

        let upgrade_watcher = UpgradeWatcher {
            current_version,
            config,
            root_dir,
            state: ComponentState::Uninitialized,
            next_upgrade,
            network_name,
            metadata_scan_id: 0,
            chainspecs,
            announced_protocols: BTreeSet::new(),
        };

        Ok(upgrade_watcher)
    }

    pub(crate) fn should_upgrade_after(&self, era_id: EraId) -> bool {
        self.next_upgrade
            .as_ref()
            .is_some_and(|upgrade| upgrade.activation_point().should_upgrade(&era_id))
    }

    pub(crate) fn next_upgrade_activation_point(&self) -> Option<EraId> {
        self.next_upgrade
            .map(|next_upgrade| next_upgrade.activation_point().era_id())
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

    fn check_for_next_upgrade<REv>(&mut self, effect_builder: EffectBuilder<REv>) -> Effects<Event>
    where
        REv: From<UpgradeWatcherAnnouncement> + Send,
    {
        self.metadata_scan_id = self.metadata_scan_id.wrapping_add(1);
        let scan_id = self.metadata_scan_id;
        let root_dir = self.root_dir.clone();
        let current_version = self.current_version;
        let network_name = self.network_name.clone();
        let mut effects = async move {
            let (maybe_next_upgrade, metadata) = task::spawn_blocking(move || {
                // Metadata validation must not change consensus upgrade selection.
                let next = next_upgrade(root_dir.clone(), current_version);
                let metadata = installed_chainspecs(&root_dir, &network_name, current_version);
                (next, metadata)
            })
            .await
            .unwrap_or_else(|error| {
                warn!(%error, "failed to join tokio task");
                (
                    None,
                    Err(format!("failed to scan installed chainspecs: {error}")),
                )
            });
            effect_builder
                .upgrade_watcher_announcement(maybe_next_upgrade)
                .await;
            metadata
        }
        .event(move |metadata| Event::GotStagedChainspecs {
            scan_id,
            metadata: Box::new(metadata),
        });

        effects.extend(
            effect_builder
                .set_timeout(self.config.upgrade_check_interval.into())
                .event(|_| Event::CheckForNextUpgrade),
        );

        effects
    }

    fn handle_got_chainspecs<REv>(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        scan_id: u64,
        metadata: Result<StagedChainspecs, String>,
    ) -> Effects<Event>
    where
        REv: From<UpgradeWatcherAnnouncement> + Send,
    {
        // A slow earlier scan must not overwrite metadata from a later rescan.
        if scan_id != self.metadata_scan_id {
            return Effects::new();
        }
        let mut effects = Effects::new();
        match &metadata {
            Ok(chainspecs) => {
                let staged_protocols = chainspecs.keys().copied().collect::<BTreeSet<_>>();
                for version in staged_protocols
                    .difference(&self.announced_protocols)
                    .copied()
                {
                    effects.extend(effect_builder.announce_staged_protocol(version).ignore());
                }
                // Edits and removals refresh source documents but do not announce staging.
                self.announced_protocols = staged_protocols;
            }
            Err(error) => warn!(%error, "invalid installed chainspec metadata"),
        }
        // Publish effects only after installing the snapshot, so announced versions can
        // immediately be fetched without copying source documents on the reactor thread.
        self.chainspecs = metadata;
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
                Event::Request(_)
                | Event::CheckForNextUpgrade
                | Event::GotNextUpgrade(_)
                | Event::GotStagedChainspecs { .. } => {
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
                Event::Request(UpgradeWatcherRequest::NextUpgrade(responder)) => {
                    responder.respond(self.next_upgrade).ignore()
                }
                Event::Request(UpgradeWatcherRequest::StagedProtocolVersions(responder)) => {
                    let versions = self
                        .chainspecs
                        .as_ref()
                        .map(|chainspecs| chainspecs.keys().copied().collect())
                        .map_err(Clone::clone);
                    responder.respond(versions).ignore()
                }
                Event::Request(UpgradeWatcherRequest::FutureChainspec {
                    protocol_version,
                    responder,
                }) => {
                    let source = self
                        .chainspecs
                        .as_ref()
                        .map(|chainspecs| chainspecs.get(&protocol_version).cloned())
                        .map_err(Clone::clone);
                    responder.respond(source).ignore()
                }
                Event::GotStagedChainspecs { scan_id, metadata } => {
                    self.handle_got_chainspecs(effect_builder, scan_id, *metadata)
                }
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
    #[serde(default)]
    evm: UpgradeEvmConfig,
}

/// Reads the activation flag and asset manifest, allowing unrelated future EVM fields.
#[derive(Default, Deserialize)]
#[serde(default)]
struct UpgradeEvmConfig {
    enabled: bool,
    preinstalls: Vec<(evm::Address, evm::Hash)>,
}

impl UpgradePoint {
    /// Parses a chainspec file at the given path as an `UpgradePoint`.
    fn from_chainspec_path<P: AsRef<Path> + fmt::Debug>(path: P) -> Result<Self, Error> {
        let contents = fs::read_to_string(path.as_ref().join(CHAINSPEC_FILENAME))
            .map_err(Error::LoadUpgradePoint)?;
        Ok(toml::from_str(&contents)?)
    }
}

/// Identify future chainspecs without interpreting engine-specific or unrelated settings.
#[derive(Deserialize)]
struct InstalledChainspec {
    protocol: InstalledProtocol,
    network: InstalledNetwork,
}

#[derive(Deserialize)]
struct InstalledProtocol {
    version: ProtocolVersion,
}

#[derive(Deserialize)]
struct InstalledNetwork {
    name: String,
}

fn read_optional_source(path: &Path) -> Result<Option<Bytes>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes.into())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("{}: {error}", path.display())),
    }
}

fn installed_chainspecs(
    root: &Path,
    network_name: &str,
    current_version: ProtocolVersion,
) -> Result<StagedChainspecs, String> {
    let mut future = BTreeMap::new();
    for entry in fs::read_dir(root).map_err(|error| format!("{}: {error}", root.display()))? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name().to_string_lossy().replace('_', ".");
        let Ok(version) = ProtocolVersion::from_str(&name) else {
            continue;
        };
        if version <= current_version {
            continue;
        }
        // Follow installed version-directory symlinks just as upgrade selection does.
        let directory = entry.path();
        if !fs::metadata(&directory)
            .map_err(|error| format!("{}: {error}", directory.display()))?
            .is_dir()
        {
            continue;
        }
        let path = directory.join(CHAINSPEC_FILENAME);
        let read = || -> Result<ChainspecRawBytes, String> {
            let contents = fs::read(&path).map_err(|error| error.to_string())?;
            let installed: InstalledChainspec =
                toml::from_str(std::str::from_utf8(&contents).map_err(|error| error.to_string())?)
                    .map_err(|error| error.to_string())?;
            if installed.protocol.version != version {
                return Err(format!(
                    "directory version {version} disagrees with protocol version {}",
                    installed.protocol.version
                ));
            }
            if installed.network.name != network_name {
                return Err(format!(
                    "network {} disagrees with running network {network_name}",
                    installed.network.name
                ));
            }
            Ok(ChainspecRawBytes::new(
                contents.into(),
                read_optional_source(&directory.join(CHAINSPEC_ACCOUNTS_FILENAME))?,
                read_optional_source(&directory.join(CHAINSPEC_GLOBAL_STATE_FILENAME))?,
            ))
        };
        let source = read().map_err(|error| format!("{}: {error}", path.display()))?;
        if future.insert(version, Arc::new(source)).is_some() {
            return Err(format!("duplicate installed protocol version {version}"));
        }
    }
    Ok(future)
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
    let max_version = ProtocolVersion::from_parts(u32::MAX, u32::MAX, u32::MAX);

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
            warn!(subdir=%subdir.display(), %error, "failed to load upgrade point");
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

    if upgrade_point.evm.enabled && !validate_evm_preinstalls(&upgrade_point.evm.preinstalls) {
        error!(protocol_version=%next_version, path=%subdir.display(),
            "withholding upgrade with duplicate EVM preinstall addresses");
        return None;
    }

    if let Err(error) = load_preinstall_bytecodes(
        &subdir,
        upgrade_point.evm.enabled,
        &upgrade_point.evm.preinstalls,
    ) {
        error!(protocol_version=%next_version, path=%subdir.display(), %error,
            "withholding upgrade with invalid EVM preinstall assets");
        return None;
    }

    Some(NextUpgrade::from(upgrade_point.protocol_config))
}

#[cfg(test)]
mod tests {
    use casper_types::{testing::TestRng, ActivationPoint};

    use super::*;
    use crate::{logging, utils::Loadable};

    const V0_0_0: ProtocolVersion = ProtocolVersion::from_parts(0, 0, 0);
    const V0_9_9: ProtocolVersion = ProtocolVersion::from_parts(0, 9, 9);
    const V1_0_0: ProtocolVersion = ProtocolVersion::from_parts(1, 0, 0);
    const V1_0_3: ProtocolVersion = ProtocolVersion::from_parts(1, 0, 3);
    const V1_2_3: ProtocolVersion = ProtocolVersion::from_parts(1, 2, 3);
    const V2_2_2: ProtocolVersion = ProtocolVersion::from_parts(2, 2, 2);

    fn stage_preinstall(root: &Path, enabled: bool) -> PathBuf {
        let dir = root.join("1_0_0");
        fs::create_dir_all(dir.join("preinstalls")).unwrap();
        let address = evm::Address::new([0xaa; 20]);
        let hash = evm::Hash::new(alloy_primitives::keccak256([0x00]).0);
        // Intentionally use a future spec and unknown fields: only protocol and hashes matter.
        let text = format!(
            r#"
[protocol]
version = "1.0.0"
hard_reset = false
activation_point = 10
[evm]
enabled = {enabled}
spec = "future_fork"
future_feature = true
preinstalls = [["{address}", "{hash}"]]
[future_subsystem]
setting = "unknown"
"#
        );
        fs::write(dir.join(CHAINSPEC_FILENAME), text).unwrap();
        dir.join("preinstalls")
            .join(format!("{}.bin", hash.to_hex_string()))
    }

    #[test]
    fn preinstall_assets_are_validated_only_when_evm_is_enabled() {
        let mut rng = crate::new_rng();
        for enabled in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = stage_preinstall(dir.path(), enabled);
            // An invalid nearest package must not cause the watcher to skip to a later version.
            install_chainspec(&mut rng, dir.path(), V1_0_3);
            let mut current = Chainspec::default();
            current.protocol_config.version = V0_9_9;
            let mut watcher =
                UpgradeWatcher::new(&current, Config::default(), dir.path().join("0_9_9")).unwrap();
            let expected = Some(NextUpgrade::new(
                ActivationPoint::EraId(EraId::new(10)),
                V1_0_0,
            ));
            if !enabled {
                assert_eq!(watcher.next_upgrade, expected);
                continue;
            }
            assert!(watcher.next_upgrade.is_none());
            let mut scan = || {
                let _ =
                    watcher.handle_got_next_upgrade(next_upgrade(dir.path().to_path_buf(), V0_9_9));
                watcher.next_upgrade
            };
            fs::write(&path, [0x00]).unwrap();
            assert_eq!(scan(), expected);
            for invalid in [vec![], vec![0x01]] {
                fs::write(&path, invalid).unwrap();
                assert!(scan().is_none());
                fs::write(&path, [0x00]).unwrap();
                assert_eq!(scan(), expected);
            }
            fs::remove_file(&path).unwrap();
            assert!(scan().is_none());
        }
    }

    #[test]
    fn staged_preinstall_manifest_rejects_duplicate_addresses_and_invalid_hashes() {
        let dir = tempfile::tempdir().unwrap();
        let bytecode_path = stage_preinstall(dir.path(), true);
        fs::write(bytecode_path, [0x00]).unwrap();
        let path = dir.path().join("1_0_0").join(CHAINSPEC_FILENAME);
        let text = fs::read_to_string(&path).unwrap();
        let hash = evm::Hash::new(alloy_primitives::keccak256([0x00]).0);
        assert!(next_upgrade(dir.path().to_path_buf(), V0_9_9).is_some());
        for address in ["aa".repeat(20), "AA".repeat(20)] {
            let duplicate = text.replace("\"]]", &format!("\"], [\"0x{address}\", \"{hash}\"]]"));
            fs::write(&path, duplicate).unwrap();
            assert!(next_upgrade(dir.path().to_path_buf(), V0_9_9).is_none());
        }
        fs::write(&path, text.replace(&hash.to_string(), "0x00")).unwrap();
        assert!(UpgradePoint::from_chainspec_path(dir.path().join("1_0_0")).is_err());
        fs::write(path, [0xff]).unwrap();
        assert!(next_upgrade(dir.path().to_path_buf(), V0_9_9).is_none());
    }

    fn source(chainspec: &Chainspec) -> ChainspecRawBytes {
        ChainspecRawBytes::new(
            toml::to_string_pretty(chainspec)
                .unwrap()
                .into_bytes()
                .into(),
            None,
            None,
        )
    }

    fn write_chainspec(root: &Path, chainspec: &Chainspec) -> PathBuf {
        let dir = root.join(dir_name_from_version(chainspec.protocol_config.version));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CHAINSPEC_FILENAME);
        fs::write(&path, source(chainspec).chainspec_bytes()).unwrap();
        path
    }

    #[test]
    fn chainspec_discovery_preserves_complete_documents_and_refreshes_all_versions() {
        let root = tempfile::tempdir().unwrap();
        let mut current = Chainspec::random(&mut TestRng::new());
        current.protocol_config.version = V1_0_0;
        current.network_config.name = "snapshot-test-network".to_string();
        let scan =
            || installed_chainspecs(root.path(), &current.network_config.name, V1_0_0).unwrap();
        assert!(scan().is_empty());

        let mut future = current.clone();
        future.protocol_config.version = V1_0_3;
        future.protocol_config.activation_point = ActivationPoint::EraId(EraId::new(100));
        let path = write_chainspec(root.path(), &future);
        let accounts = b"# complete account source documents\n";
        let state = b"# complete global state source documents\n";
        fs::write(
            path.parent().unwrap().join(CHAINSPEC_ACCOUNTS_FILENAME),
            accounts,
        )
        .unwrap();
        fs::write(
            path.parent().unwrap().join(CHAINSPEC_GLOBAL_STATE_FILENAME),
            state,
        )
        .unwrap();
        let one = scan();
        let found = &one[&V1_0_3];
        assert_eq!(found.chainspec_bytes(), fs::read(&path).unwrap());
        assert_eq!(
            found.maybe_genesis_accounts_bytes(),
            Some(accounts.as_slice())
        );
        assert_eq!(found.maybe_global_state_bytes(), Some(state.as_slice()));

        // Preserve unknown future settings and edits without changing version or activation.
        future.core_config.validator_slots = 500;
        let mut edited = toml::to_string_pretty(&future).unwrap();
        edited.push_str("\n[future_engine]\nunknown_setting = 'preserved verbatim'\n");
        fs::write(&path, &edited).unwrap();
        assert_eq!(scan()[&V1_0_3].chainspec_bytes(), edited.as_bytes());
        assert_eq!(
            next_upgrade(root.path().to_path_buf(), V1_0_0),
            Some(NextUpgrade::from(future.protocol_config.clone()))
        );
        fs::remove_file(path.parent().unwrap().join(CHAINSPEC_GLOBAL_STATE_FILENAME)).unwrap();
        assert!(scan()[&V1_0_3].maybe_global_state_bytes().is_none());

        for version in [V2_2_2, V1_2_3] {
            future.protocol_config.version = version;
            write_chainspec(root.path(), &future);
        }
        assert_eq!(
            scan().keys().copied().collect::<Vec<_>>(),
            vec![V1_0_3, V1_2_3, V2_2_2]
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert_eq!(scan().len(), 2);
        fs::remove_dir_all(root.path().join(dir_name_from_version(V1_2_3))).unwrap();
        fs::remove_dir_all(root.path().join(dir_name_from_version(V2_2_2))).unwrap();
        assert!(scan().is_empty());
    }

    #[test]
    fn invalid_installed_chainspecs_do_not_change_upgrade_selection() {
        let root = tempfile::tempdir().unwrap();
        let mut current = Chainspec::random(&mut TestRng::new());
        current.protocol_config.version = V1_0_0;
        current.network_config.name = "snapshot-test-network".to_string();
        let mut future = current.clone();
        future.protocol_config.version = V1_0_3;
        let path = write_chainspec(root.path(), &future);
        let valid = fs::read_to_string(&path).unwrap();
        let scan = || installed_chainspecs(root.path(), &current.network_config.name, V1_0_0);
        fs::write(
            &path,
            valid.replace("snapshot-test-network", "wrong-network"),
        )
        .unwrap();
        assert!(scan()
            .unwrap_err()
            .contains("disagrees with running network"));
        assert_eq!(
            next_upgrade(root.path().to_path_buf(), V1_0_0),
            Some(NextUpgrade::from(future.protocol_config.clone()))
        );
        fs::write(
            &path,
            valid.replace("version = \"1.0.3\"", "version = \"1.2.3\""),
        )
        .unwrap();
        assert!(scan()
            .unwrap_err()
            .contains("disagrees with protocol version"));
        assert_eq!(next_upgrade(root.path().to_path_buf(), V1_0_0), None);
        fs::write(&path, [0xff]).unwrap();
        assert!(scan().is_err());
        assert_eq!(next_upgrade(root.path().to_path_buf(), V1_0_0), None);
        fs::write(&path, valid).unwrap();
        let accounts = path.parent().unwrap().join(CHAINSPEC_ACCOUNTS_FILENAME);
        fs::create_dir(&accounts).unwrap();
        assert!(scan().is_err());
        assert_eq!(
            next_upgrade(root.path().to_path_buf(), V1_0_0),
            Some(NextUpgrade::from(future.protocol_config))
        );
    }

    fn test_effect_builder() -> (
        EffectBuilder<MainEvent>,
        &'static crate::reactor::Scheduler<MainEvent>,
    ) {
        use crate::reactor::{EventQueueHandle, QueueKind, Scheduler};
        let scheduler = crate::utils::leak(Scheduler::<MainEvent>::new(QueueKind::weights(), None));
        (
            EffectBuilder::new(EventQueueHandle::without_shutdown(scheduler)),
            scheduler,
        )
    }

    async fn staged_announcements(
        effects: Effects<Event>,
        scheduler: &crate::reactor::Scheduler<MainEvent>,
    ) -> Vec<ProtocolVersion> {
        for effect in effects {
            effect.await;
        }
        let mut versions = Vec::new();
        while scheduler.item_count() > 0 {
            let ((_, event), _) = scheduler.pop().await;
            let MainEvent::UpgradeWatcherAnnouncement(UpgradeWatcherAnnouncement::StagedProtocol(
                version,
            )) = event
            else {
                panic!("unexpected staging announcement: {event:?}");
            };
            versions.push(version);
        }
        versions.sort();
        versions
    }

    #[tokio::test]
    async fn chainspec_refresh_is_atomic_and_ignores_out_of_order_scans() {
        let root = tempfile::tempdir().unwrap();
        let current = Chainspec::random(&mut TestRng::new());
        let mut watcher =
            UpgradeWatcher::new(&current, Config::default(), root.path().join("current")).unwrap();
        let original = watcher.chainspecs.clone().unwrap();
        let mut updated = original.clone();
        let version = ProtocolVersion::from_parts(u32::MAX, 0, 0);
        updated.insert(version, Arc::new(source(&current)));
        let (effect_builder, scheduler) = test_effect_builder();
        watcher.metadata_scan_id = 2;
        let effects = watcher.handle_got_chainspecs(effect_builder, 2, Ok(updated.clone()));
        assert_eq!(
            staged_announcements(effects, scheduler).await,
            vec![version]
        );
        let effects = watcher.handle_got_chainspecs(effect_builder, 1, Ok(original));
        assert!(staged_announcements(effects, scheduler).await.is_empty());
        assert_eq!(watcher.chainspecs, Ok(updated));
        let effects = watcher.handle_got_chainspecs(
            effect_builder,
            2,
            Err("unreadable installed chainspec".to_string()),
        );
        assert!(staged_announcements(effects, scheduler).await.is_empty());
        assert!(watcher.chainspecs.is_err());
    }

    #[tokio::test]
    async fn announces_only_newly_staged_versions_after_caching_documents() {
        let root = tempfile::tempdir().unwrap();
        let mut current = Chainspec::random(&mut TestRng::new());
        current.protocol_config.version = V1_0_0;
        current.network_config.name = "staging-test-network".to_string();
        let mut future = current.clone();
        future.protocol_config.version = V1_0_3;
        let path = write_chainspec(root.path(), &future);
        let mut watcher =
            UpgradeWatcher::new(&current, Config::default(), root.path().join("current")).unwrap();
        let (effect_builder, scheduler) = test_effect_builder();
        let scan = || installed_chainspecs(root.path(), &current.network_config.name, V1_0_0);

        // Include versions already installed when the node started, then suppress repeated scans.
        let effects = watcher.handle_got_chainspecs(effect_builder, 0, scan());
        assert!(watcher.chainspecs.as_ref().unwrap().contains_key(&V1_0_3));
        assert_eq!(staged_announcements(effects, scheduler).await, vec![V1_0_3]);
        let effects = watcher.handle_got_chainspecs(effect_builder, 0, scan());
        assert!(staged_announcements(effects, scheduler).await.is_empty());

        // Announce every newly staged version, including ones after the nearest upgrade.
        future.protocol_config.version = V2_2_2;
        write_chainspec(root.path(), &future);
        future.protocol_config.version = V1_2_3;
        write_chainspec(root.path(), &future);
        let effects = watcher.handle_got_chainspecs(effect_builder, 0, scan());
        assert_eq!(
            staged_announcements(effects, scheduler).await,
            vec![V1_2_3, V2_2_2]
        );

        // Edits refresh data and removals forget versions without emitting staging events.
        let edited = format!("{}\n# edited", fs::read_to_string(&path).unwrap());
        fs::write(&path, &edited).unwrap();
        let effects = watcher.handle_got_chainspecs(effect_builder, 0, scan());
        assert!(staged_announcements(effects, scheduler).await.is_empty());
        assert_eq!(
            watcher.chainspecs.as_ref().unwrap()[&V1_0_3].chainspec_bytes(),
            edited.as_bytes()
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        let effects = watcher.handle_got_chainspecs(effect_builder, 0, scan());
        assert!(staged_announcements(effects, scheduler).await.is_empty());
        assert!(!watcher.chainspecs.as_ref().unwrap().contains_key(&V1_0_3));

        // Staging a previously removed version is a new staging operation.
        future.protocol_config.version = V1_0_3;
        write_chainspec(root.path(), &future);
        let effects = watcher.handle_got_chainspecs(effect_builder, 0, scan());
        assert_eq!(staged_announcements(effects, scheduler).await, vec![V1_0_3]);
    }

    #[tokio::test]
    async fn staged_chainspec_requests_reflect_cached_versions_across_refreshes() {
        use crate::effect::Responder;
        use futures::channel::oneshot;

        async fn request_versions(
            watcher: &mut UpgradeWatcher,
            effect_builder: EffectBuilder<MainEvent>,
            rng: &mut NodeRng,
        ) -> Result<Vec<ProtocolVersion>, String> {
            let (sender, receiver) = oneshot::channel();
            let effects = watcher.handle_event(
                effect_builder,
                rng,
                Event::Request(UpgradeWatcherRequest::StagedProtocolVersions(
                    Responder::without_shutdown(sender),
                )),
            );
            for effect in effects {
                effect.await;
            }
            receiver.await.unwrap()
        }

        let root = tempfile::tempdir().unwrap();
        let (mut current, future_source) =
            <(Chainspec, ChainspecRawBytes)>::from_resources("local");
        current.protocol_config.version = V1_0_0;
        let future_version = V1_0_3;
        let mut watcher =
            UpgradeWatcher::new(&current, Config::default(), root.path().join("current")).unwrap();
        watcher.state = ComponentState::Initialized;
        let cached = Arc::new(future_source);
        let (effect_builder, scheduler) = test_effect_builder();
        let effects = watcher.handle_got_chainspecs(
            effect_builder,
            0,
            Ok(BTreeMap::from([
                (future_version, cached.clone()),
                (V1_2_3, Arc::new(source(&current))),
            ])),
        );
        staged_announcements(effects, scheduler).await;
        let mut received = Vec::new();
        let mut rng = TestRng::new();
        assert_eq!(
            request_versions(&mut watcher, effect_builder, &mut rng)
                .await
                .unwrap(),
            vec![future_version, V1_2_3],
        );
        for version in [future_version, future_version, V1_0_0, V2_2_2] {
            let (sender, receiver) = oneshot::channel();
            let effects = watcher.handle_event(
                effect_builder,
                &mut rng,
                Event::Request(UpgradeWatcherRequest::FutureChainspec {
                    protocol_version: version,
                    responder: Responder::without_shutdown(sender),
                }),
            );
            for effect in effects {
                effect.await;
            }
            let source = receiver.await.unwrap().unwrap();
            if version == future_version {
                let source = source.unwrap();
                assert!(Arc::ptr_eq(&cached, &source));
                received.push(source);
            } else {
                assert!(source.is_none());
            }
        }
        watcher.metadata_scan_id = 1;
        let effects = watcher.handle_got_chainspecs(effect_builder, 1, Ok(BTreeMap::new()));
        assert!(staged_announcements(effects, scheduler).await.is_empty());
        assert!(request_versions(&mut watcher, effect_builder, &mut rng)
            .await
            .unwrap()
            .is_empty());
        let error = "unreadable installed chainspec".to_string();
        let effects = watcher.handle_got_chainspecs(effect_builder, 1, Err(error.clone()));
        assert!(staged_announcements(effects, scheduler).await.is_empty());
        assert_eq!(
            request_versions(&mut watcher, effect_builder, &mut rng).await,
            Err(error)
        );
        for source in received {
            assert!(Arc::ptr_eq(&cached, &source));
        }
    }

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

        let pretty = toml::to_string_pretty(&chainspec);
        fs::write(path, pretty.expect("should encode to toml")).expect("should install chainspec");
        chainspec
    }

    #[test]
    fn should_get_next_upgrade() {
        let tempdir = tempfile::tempdir().expect("should create temp dir");

        let next_point = |current_version: ProtocolVersion| {
            next_upgrade(tempdir.path().to_path_buf(), current_version).unwrap()
        };

        let mut rng = crate::new_rng();

        let mut current = ProtocolVersion::from_parts(1, 9, 9);
        let v2_0_0 = ProtocolVersion::from_parts(2, 0, 0);
        let chainspec_v2_0_0 = install_chainspec(&mut rng, tempdir.path(), v2_0_0);
        assert_eq!(next_point(current), chainspec_v2_0_0.protocol_config.into());

        current = v2_0_0;
        let v2_0_3 = ProtocolVersion::from_parts(2, 0, 3);
        let chainspec_v2_0_3 = install_chainspec(&mut rng, tempdir.path(), v2_0_3);
        assert_eq!(next_point(current), chainspec_v2_0_3.protocol_config.into());

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
        let (chainspec, _raw) = <(Chainspec, ChainspecRawBytes)>::from_resources("local");
        let mut upgrade_watcher =
            UpgradeWatcher::new(&chainspec, Config::default(), tempdir.path()).unwrap();
        assert!(upgrade_watcher.next_upgrade.is_none());

        let next_upgrade = NextUpgrade::new(
            ActivationPoint::EraId(EraId::MAX),
            ProtocolVersion::from_parts(9, 9, 9),
        );
        let _ = upgrade_watcher.handle_got_next_upgrade(Some(next_upgrade));
        assert_eq!(Some(next_upgrade), upgrade_watcher.next_upgrade);

        let _ = upgrade_watcher.handle_got_next_upgrade(None);
        assert!(upgrade_watcher.next_upgrade.is_none());
    }
}
