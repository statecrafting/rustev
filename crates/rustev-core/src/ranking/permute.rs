//! `rustev.permute/1` (spec 039, 3.8.4), using spec 002's identity builder.
use rustev_contract::canonical::{CanonicalError, canonical_bytes, tagged_digest};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermuteError {
    Canonical(CanonicalError),
    DigestEncoding,
    Overflow,
}

/// Distinguishes integer permutation indices from string comparison keys.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(untagged)]
pub enum DrawKey<'a> {
    Permutation(u64),
    Comparison(&'a str),
}

/// An unbiased draw in 0..=i, also exposing rejected attempts for vectors.
pub fn draw(
    seed: &str,
    step: &str,
    instance: &[String],
    j: DrawKey<'_>,
    i: u64,
) -> Result<(u64, u64), PermuteError> {
    let range = u128::from(i) + 1;
    let space = 1u128 << 64;
    let limit = (space / range) * range;
    let mut attempt = 0u64;
    loop {
        // Spec 039, 3.8.4: instance is the contract's array of strings
        // (Vec<String>, empty at the step root); j is an integer for a
        // permutation index and a string for a pairwise comparison key.
        let bytes = canonical_bytes(&(seed, step, instance, j, i, attempt))
            .map_err(PermuteError::Canonical)?;
        let digest = tagged_digest("rustev.permute/1", &bytes);
        let prefix = digest.get(7..23).ok_or(PermuteError::DigestEncoding)?;
        // The first sixteen hex digits encode the first eight digest bytes
        // in big-endian order, without a second hashing implementation.
        let v = u64::from_str_radix(prefix, 16).map_err(|_| PermuteError::DigestEncoding)?;
        if u128::from(v) < limit {
            return Ok(((u128::from(v) % range) as u64, attempt));
        }
        attempt = attempt.checked_add(1).ok_or(PermuteError::Overflow)?;
    }
}

/// Pool-position permutation; callers retain candidate ids in pool order.
pub fn permutation(
    size: usize,
    seed: &str,
    step: &str,
    instance: &[String],
    j: u64,
) -> Result<Vec<usize>, PermuteError> {
    let mut positions: Vec<_> = (0..size).collect();
    for i in (1..size).rev() {
        let index = u64::try_from(i).map_err(|_| PermuteError::Overflow)?;
        let (r, _) = draw(seed, step, instance, DrawKey::Permutation(j), index)?;
        let r = usize::try_from(r).map_err(|_| PermuteError::Overflow)?;
        // draw guarantees r <= i, and i < size, so swap cannot panic.
        positions.swap(i, r);
    }
    Ok(positions)
}

/// The pairwise presentation bit (i = 1, string comparison key).
pub fn presentation_bit(
    seed: &str,
    step: &str,
    instance: &[String],
    comparison: &str,
) -> Result<bool, PermuteError> {
    draw(seed, step, instance, DrawKey::Comparison(comparison), 1).map(|(v, _)| v == 1)
}
