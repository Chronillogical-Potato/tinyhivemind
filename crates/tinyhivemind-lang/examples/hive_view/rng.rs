//! A seeded `SplitMix64` generator, so every simulated run repeats exactly.
//!
//! No `rand` dependency: the example needs reproducible draws, not quality
//! randomness, and `SplitMix64` is a dozen lines with a published reference.

/// Parts per million, the unit every probability in `sim.json` uses.
pub(crate) const PPM: u64 = 1_000_000;

/// `SplitMix64` (Steele, Lea and Flood, 2014).
#[derive(Clone, Debug)]
pub(crate) struct Rng(u64);

impl Rng {
    /// A generator for one stream, derived from a seed and a stream label.
    ///
    /// Mixing the label in means a seat's draws do not depend on how many
    /// draws another seat made first.
    pub(crate) fn stream(seed: u64, label: &str) -> Self {
        let mut state = seed ^ 0x9E37_79B9_7F4A_7C15;
        for byte in label.bytes() {
            state = mix(state ^ u64::from(byte));
        }
        Self(state)
    }

    /// The next 64 random bits.
    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        mix(self.0)
    }

    /// A value in `0..bound`; `0` when `bound` is zero.
    pub(crate) fn below(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        self.next_u64() % bound
    }

    /// A value in `low..=high` (parts per million or any integer range).
    pub(crate) fn between(&mut self, low: u64, high: u64) -> u64 {
        if high <= low {
            return low;
        }
        low + self.below(high - low + 1)
    }

    /// True with probability `ppm` parts per million.
    pub(crate) fn chance(&mut self, ppm: u64) -> bool {
        self.below(PPM) < ppm
    }
}

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
