//! The synthetic encoder and synthetic heads (spec 011, 3.19): a
//! deterministic, dependency-free stand-in for the ONNX encoder that needs
//! no runtime and is bitwise repeatable. It proves mechanics only; its
//! outputs mean nothing about any text.
//!
//! **Encoder.** Each token id `t` is hashed with SplitMix64,
//! `h = splitmix64(t)`. Its token vector is zero except component
//! `h mod hidden_size`, which holds the fixed f32 weight
//! `w(h) = ((h >> 40) as f32) / 2^23 - 1` (exact: 24 bits, in [-1, 1)).
//! The result is the `tokens x hidden_size` last hidden state that
//! `mean_masked` pooling then reduces, as for the ONNX encoder.
//!
//! **Heads.** Value `k` of a head generated from `seed` is
//! `w(splitmix64(seed + k))` for the `options x hidden_size` weights
//! followed by the `options` biases (3.9.3), written little-endian. They
//! are generated from the seed by this formula, never fitted to data.

use crate::bundle::VerifiedBundle;
use crate::manifest::EncoderKind;

/// The SplitMix64 output function of `x`.
pub fn splitmix64(x: u64) -> u64 {
    let mut z = x.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The fixed f32 weight of a hash: its top 24 bits mapped exactly onto
/// `[-1, 1)`.
pub fn weight(h: u64) -> f32 {
    (h >> 40) as f32 / 8_388_608.0 - 1.0
}

/// The synthetic encoder of a `kind: synthetic` bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntheticEncoder {
    hidden: usize,
}

impl SyntheticEncoder {
    /// The encoder of `bundle`, or `None` when its encoder is not
    /// synthetic.
    pub fn for_bundle(bundle: &VerifiedBundle) -> Option<Self> {
        let e = &bundle.manifest.encoder;
        (e.kind == EncoderKind::Synthetic).then_some(Self {
            hidden: e.hidden_size as usize,
        })
    }

    /// An encoder of `hidden` dimensions (at least one).
    pub fn new(hidden: usize) -> Option<Self> {
        (hidden > 0).then_some(Self { hidden })
    }

    pub fn hidden_size(&self) -> usize {
        self.hidden
    }

    /// The `tokens x hidden` token vectors of `ids`.
    pub fn encode(&self, ids: &[u32]) -> Vec<f32> {
        let mut out = vec![0f32; ids.len() * self.hidden];
        for (t, &id) in ids.iter().enumerate() {
            let h = splitmix64(u64::from(id));
            out[t * self.hidden + (h % self.hidden as u64) as usize] = weight(h);
        }
        out
    }
}

/// The weights file of a synthetic head (3.19.2) for `options` options over
/// `hidden` dimensions, generated from `seed`.
pub fn head_weights(seed: u64, options: usize, hidden: usize) -> Vec<u8> {
    let n = options * hidden + options;
    (0..n as u64)
        .flat_map(|k| weight(splitmix64(seed.wrapping_add(k))).to_le_bytes())
        .collect()
}
