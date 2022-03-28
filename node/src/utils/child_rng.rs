//! Sub-RNG creation trait.

use rand::{rngs::StdRng, Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;

/// Create RNGs from existing RNGs.
pub(crate) trait ChildRng {
    /// Creates a new RNG by seeding it from an existing one.
    ///
    /// This method differs from `SeedableRng::from_rng` in that it guarantees reproducibility (see
    /// comments in `SeedableRng::from_rng`) and is restricted to initializing RNGs of the same
    /// type. It also does not viable the one-true-RNG rule of `TestRng` in implementations.
    ///
    /// Additionally, it cannot fail.
    fn new_child(&mut self) -> Self;
}

impl ChildRng for StdRng {
    fn new_child(&mut self) -> Self {
        let mut seed = <StdRng as SeedableRng>::Seed::default();
        self.fill(&mut seed);

        StdRng::from_seed(seed)
    }
}

impl ChildRng for ChaCha20Rng {
    fn new_child(&mut self) -> Self {
        let mut seed = <ChaCha20Rng as SeedableRng>::Seed::default();
        self.fill(&mut seed);

        ChaCha20Rng::from_seed(seed)
    }
}

#[cfg(test)]
mod tests {
    use rand::{rngs::StdRng, Rng, SeedableRng};

    use crate::testing::TestRng;

    use super::ChildRng;

    // Note: Using `TestRng` to test `StdRng` and itself is necessary here, if slightly ironic.

    /// Tests that a random number generator creates different values in parent and child.
    fn rng_forks_properly<R: ChildRng + Rng>(mut parent: R) {
        let mut child = parent.new_child();

        let mut parent_buf = [0u8; 32];
        let mut child_buf = [0u8; 32];

        parent.fill(&mut parent_buf);
        child.fill(&mut child_buf);

        assert_ne!(parent_buf, child_buf);
    }

    #[test]
    fn stdrng_forks_properly() {
        let mut test_rng = TestRng::default();
        let seed: <StdRng as SeedableRng>::Seed = test_rng.gen();

        let std_rng = StdRng::from_seed(seed);

        rng_forks_properly(std_rng)
    }

    #[test]
    fn testrng_forks_properly() {
        let test_rng = TestRng::default();

        rng_forks_properly(test_rng)
    }
}
