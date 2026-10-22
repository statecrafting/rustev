//! The bootstrap generator `xoshiro256starstar-splitmix64/1` (spec 029,
//! 3.4.2 to 3.4.4 and 3.4.6).
//!
//! Implemented in-crate on wrapping `u64` arithmetic, so its stream can
//! never change with a dependency's version. A later change of any step is
//! a new generator name; this one never changes.

use rustev_contract::canonical::tagged_digest;

/// The generator's name in a repeat configuration.
pub const GENERATOR: &str = "xoshiro256starstar-splitmix64/1";
/// The tag of the stream-seed digest (3.4.3).
pub const STREAM_TAG: &str = "rustev.bootstrap/1";

/// SplitMix64 with state `x`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitMix64 {
    pub x: u64,
}

impl SplitMix64 {
    pub fn next_u64(&mut self) -> u64 {
        self.x = self.x.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.x;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// xoshiro256\*\* with state `s[0..4]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xoshiro256StarStar {
    pub s: [u64; 4],
}

impl Xoshiro256StarStar {
    /// State from four consecutive SplitMix64 outputs.
    pub fn from_splitmix(seed: u64) -> Self {
        let mut sm = SplitMix64 { x: seed };
        let s = [sm.next_u64(), sm.next_u64(), sm.next_u64(), sm.next_u64()];
        Xoshiro256StarStar { s }
    }

    pub fn next_u64(&mut self) -> u64 {
        let s = &mut self.s;
        let result = s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = s[1] << 17;
        s[2] ^= s[0];
        s[3] ^= s[1];
        s[1] ^= s[2];
        s[0] ^= s[3];
        s[2] ^= t;
        s[3] = s[3].rotate_left(45);
        result
    }

    /// A uniform index in `[0, n)` by rejection (3.4.4). `n` is at least 1.
    pub fn index(&mut self, n: u64) -> u64 {
        assert!(n >= 1, "an index needs a non-empty range");
        // 2^64 mod n, in u64.
        let rem = (u64::MAX % n + 1) % n;
        loop {
            let u = self.next_u64();
            if rem != 0 && u >= 0u64.wrapping_sub(rem) {
                continue;
            }
            return u % n;
        }
    }
}

/// The SplitMix64 starting state of the stream for `label` (3.4.3): the
/// first 8 bytes, big-endian, of the SHA-256 digest in
/// `tagged_digest("rustev.bootstrap/1", seed || 0x00 || label)`.
pub fn stream_seed(seed: &str, label: &str) -> u64 {
    let mut bytes = Vec::with_capacity(seed.len() + 1 + label.len());
    bytes.extend_from_slice(seed.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(label.as_bytes());
    let digest = tagged_digest(STREAM_TAG, &bytes);
    let hex = &digest["sha256:".len().."sha256:".len() + 16];
    u64::from_str_radix(hex, 16).expect("a tagged digest is lowercase hex")
}

/// The stream for `label`.
pub fn stream(seed: &str, label: &str) -> Xoshiro256StarStar {
    Xoshiro256StarStar::from_splitmix(stream_seed(seed, label))
}

/// The seed handed to run `r` (3.4.6): the first output of the stream
/// `run/<r>`, with `r` the run's zero-based position in decimal.
pub fn run_seed(seed: &str, r: usize) -> u64 {
    stream(seed, &format!("run/{r}")).next_u64()
}
