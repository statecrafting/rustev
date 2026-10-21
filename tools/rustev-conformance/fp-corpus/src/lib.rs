//! Spec 027, 3.11.3: the differential floating-point corpus.
//!
//! Every transcendental call site reachable from a pure command (after
//! spec 047, each a call of `rustev_core::math`) runs over a fixed input
//! set of at least 100,000 values drawn as that site sees them, subnormal,
//! boundary and large-magnitude values included. The set is fixed by this
//! file: a seeded generator using integer arithmetic and correctly rounded
//! IEEE 754 basic operations only, so every target draws the same bits, and
//! `expected.json` pins the SHA-256 of the inputs as well as of the
//! outputs. A target whose input or output digest differs fails (3.11.5).
//! NaN payloads are out of scope: canonical JSON refuses non-finite
//! numbers, and no input is NaN.

use sha2::{Digest, Sha256};
use std::sync::Mutex;

/// Inputs per call site (3.11.3: at least 100,000).
pub const INPUTS: usize = 131_072;

/// A call site: its id, the source file it is in, and the function the
/// site applies.
pub struct Site {
    pub id: &'static str,
    pub file: &'static str,
    pub f: fn(f64) -> f64,
    domain: Domain,
    seed: u64,
}

#[derive(Clone, Copy)]
enum Domain {
    /// `v - max` with `max` the largest logit: every value at most 0.
    NonPositive,
    /// A finite, positive distribution mass.
    Positive,
    /// A probability: in (0, 1], with values just above 1 that a
    /// distribution within its tolerance can carry.
    Probability,
}

/// The four call sites (spec 047, section 2). `tests/sites.rs` fails when
/// the pure crates' `rustev_core::math` calls differ from this list.
pub const SITES: [Site; 4] = [
    Site {
        id: "core.softmax.exp",
        file: "crates/rustev-core/src/kinds.rs",
        f: rustev_core::math::exp,
        domain: Domain::NonPositive,
        seed: 0x0027_3113_0000_0001,
    },
    Site {
        id: "core.calibrate.ln",
        file: "crates/rustev-core/src/calibrate.rs",
        f: rustev_core::math::ln,
        domain: Domain::Positive,
        seed: 0x0027_3113_0000_0002,
    },
    Site {
        id: "eval.log_loss.ln",
        file: "crates/rustev-eval/src/metrics.rs",
        f: rustev_core::math::ln,
        domain: Domain::Probability,
        seed: 0x0027_3113_0000_0003,
    },
    Site {
        id: "eval.temperature_fit.ln",
        file: "crates/rustev-eval/src/fit.rs",
        f: rustev_core::math::ln,
        domain: Domain::Probability,
        seed: 0x0027_3113_0000_0004,
    },
];

/// SplitMix64: integer arithmetic only, the same on every target.
fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// A uniform value in [0, 1): 53 random bits scaled exactly.
fn unit(state: &mut u64) -> f64 {
    (next(state) >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// Values every domain includes where they belong: the boundaries of
/// `exp` and `ln` and the extremes of the binary64 format.
fn specials(domain: Domain) -> Vec<f64> {
    let tiny = f64::from_bits(1); // the smallest subnormal
    let big_sub = f64::from_bits(0x000F_FFFF_FFFF_FFFF); // the largest subnormal
    let below_one = f64::from_bits(1.0f64.to_bits() - 1);
    let above_one = f64::from_bits(1.0f64.to_bits() + 1);
    match domain {
        Domain::NonPositive => vec![
            0.0,
            -0.0,
            -tiny,
            -big_sub,
            -f64::MIN_POSITIVE,
            -f64::EPSILON,
            -1.0,
            -0.5,
            -708.396_418_532_264_1, // ln(f64::MIN_POSITIVE): normal results end
            -708.396_418_532_264_2,
            -709.0,
            -744.440_071_921_381_3, // results reach the smallest subnormal
            -745.133_219_101_941_1, // and round to zero below
            -745.133_219_101_941_2,
            -746.0,
            -1.0e10,
            f64::MIN,
            f64::NEG_INFINITY,
        ],
        Domain::Positive => vec![
            tiny,
            big_sub,
            f64::MIN_POSITIVE,
            f64::EPSILON,
            below_one,
            1.0,
            above_one,
            2.0,
            core::f64::consts::E,
            1.0e10,
            1.0e300,
            f64::MAX,
        ],
        Domain::Probability => vec![
            tiny,
            big_sub,
            f64::MIN_POSITIVE,
            f64::EPSILON,
            1.0e-300,
            0.5,
            below_one,
            1.0,
            above_one,
            1.0 + 1.0e-9,
        ],
    }
}

/// A site's inputs: its specials, then seeded draws until `INPUTS`.
pub fn inputs(site: &Site) -> Vec<f64> {
    let mut v = specials(site.domain);
    let mut s = site.seed;
    while v.len() < INPUTS {
        let pick = next(&mut s) % 4;
        let x = match (site.domain, pick) {
            // Where results are normal or subnormal: uniform over [-746, 0].
            (Domain::NonPositive, 0 | 1) => -746.0 * unit(&mut s),
            // Large magnitudes, uniform in the exponent.
            (Domain::NonPositive, 2) => -f64::from_bits(next(&mut s) & 0x7FEF_FFFF_FFFF_FFFF),
            // Small magnitudes near 0, where exp is near 1.
            (Domain::NonPositive, _) => -unit(&mut s) * 1.0e-3,
            // Any positive finite value, subnormals included.
            (Domain::Positive, 0 | 1) => f64::from_bits(next(&mut s) & 0x7FEF_FFFF_FFFF_FFFF),
            (Domain::Positive, 2) => unit(&mut s),
            (Domain::Positive, _) => 1.0 + (unit(&mut s) - 0.5) * 1.0e-6,
            // Probabilities: uniform, near 1, and tiny.
            (Domain::Probability, 0 | 1) => unit(&mut s),
            (Domain::Probability, 2) => 1.0 - unit(&mut s) * 1.0e-6,
            (Domain::Probability, _) => f64::from_bits(next(&mut s) % 0x3FF0_0000_0000_0000),
        };
        // A site never sees 0 for `ln` (it handles 0 before the call) or
        // a positive value for `exp`.
        let admissible = match site.domain {
            Domain::NonPositive => x <= 0.0,
            Domain::Positive | Domain::Probability => x > 0.0 && x.is_finite(),
        };
        if admissible {
            v.push(x);
        }
    }
    v.truncate(INPUTS);
    v
}

/// The SHA-256 of a site's input bits and of its output bits, each value
/// as its 8 little-endian bytes.
pub fn digests(site: &Site) -> ([u8; 32], [u8; 32]) {
    let mut i = Sha256::new();
    let mut o = Sha256::new();
    for x in inputs(site) {
        i.update(x.to_bits().to_le_bytes());
        o.update((site.f)(x).to_bits().to_le_bytes());
    }
    (i.finalize().into(), o.finalize().into())
}

/// Lowercase hex.
pub fn hex(d: &[u8; 32]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

// ---- the WebAssembly module's exports ------------------------------------
//
// Plain exports with no imports: the JavaScript runner calls
// `corpus_run(site)` and reads the two digests a byte at a time.

static LAST: Mutex<[[u8; 32]; 2]> = Mutex::new([[0; 32]; 2]);

/// The number of call sites.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn corpus_sites() -> u32 {
    SITES.len() as u32
}

/// Runs site `site` and keeps its digests; returns the inputs run, 0 for
/// an unknown site.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn corpus_run(site: u32) -> u32 {
    let Some(s) = SITES.get(site as usize) else {
        return 0;
    };
    let (i, o) = digests(s);
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = [i, o];
    INPUTS as u32
}

/// Byte `index` of the last run's input (`which` 0) or output (1) digest,
/// or `u32::MAX` (which JavaScript receives as -1) for an index out of range.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn corpus_digest_byte(which: u32, index: u32) -> u32 {
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    last.get(which as usize)
        .and_then(|d| d.get(index as usize))
        .map_or(u32::MAX, |b| u32::from(*b))
}
