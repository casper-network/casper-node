use casperlabs_types::SemVer;

use crate::engine_server::state;

impl From<state::SemVer> for SemVer {
    fn from(pb_semver: state::SemVer) -> Self {
        Self::new(pb_semver.major, pb_semver.minor, pb_semver.patch)
    }
}

impl Into<state::SemVer> for SemVer {
    fn into(self) -> state::SemVer {
        let mut res = state::SemVer::new();
        res.set_major(self.major);
        res.set_minor(self.minor);
        res.set_patch(self.patch);
        res
    }
}
