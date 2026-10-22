//! Trace and span ids (spec 028, 3.5 and 3.7): W3C sizes, never all zero.
//!
//! [`SeededIds`] is deterministic, for goldens (3.5.8); [`RandomIds`] is
//! what a sink uses unless the host supplies its own source.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

/// A source of trace and span ids.
pub trait IdSource: Send {
    /// A 16-byte trace id, never all zero.
    fn trace_id(&mut self) -> [u8; 16];
    /// An 8-byte span id, never all zero.
    fn span_id(&mut self) -> [u8; 8];
}

/// Ids from SHA-256 over a seed and a counter: the same seed gives the same
/// sequence.
#[derive(Debug, Clone)]
pub struct SeededIds {
    seed: [u8; 32],
    counter: u64,
}

impl SeededIds {
    pub fn new(seed: &[u8]) -> Self {
        SeededIds {
            seed: Sha256::digest(seed).into(),
            counter: 0,
        }
    }

    fn next(&mut self) -> [u8; 32] {
        loop {
            self.counter += 1;
            let mut h = Sha256::new();
            h.update(b"rustev-otel.ids/1");
            h.update(self.seed);
            h.update(self.counter.to_be_bytes());
            let out: [u8; 32] = h.finalize().into();
            if out[..16].iter().any(|b| *b != 0) && out[..8].iter().any(|b| *b != 0) {
                return out;
            }
        }
    }
}

impl IdSource for SeededIds {
    fn trace_id(&mut self) -> [u8; 16] {
        let mut id = [0u8; 16];
        id.copy_from_slice(&self.next()[..16]);
        id
    }

    fn span_id(&mut self) -> [u8; 8] {
        let mut id = [0u8; 8];
        id.copy_from_slice(&self.next()[..8]);
        id
    }
}

/// Ids seeded once per source from the process's randomized hasher keys,
/// the wall clock and the process id. Unpredictable enough for correlation
/// ids; they carry no secret.
#[derive(Debug, Clone)]
pub struct RandomIds(SeededIds);

impl Default for RandomIds {
    fn default() -> Self {
        let mut h = RandomState::new().build_hasher();
        h.write_u128(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        h.write_u32(std::process::id());
        let a = h.finish();
        let mut h2 = RandomState::new().build_hasher();
        h2.write_u64(a);
        let b = h2.finish();
        let mut seed = [0u8; 16];
        seed[..8].copy_from_slice(&a.to_be_bytes());
        seed[8..].copy_from_slice(&b.to_be_bytes());
        RandomIds(SeededIds::new(&seed))
    }
}

impl IdSource for RandomIds {
    fn trace_id(&mut self) -> [u8; 16] {
        self.0.trace_id()
    }

    fn span_id(&mut self) -> [u8; 8] {
        self.0.span_id()
    }
}

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
        s.push(char::from_digit(u32::from(b & 0xf), 16).unwrap_or('0'));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_ids_repeat_and_random_ids_differ() {
        let mut a = SeededIds::new(b"x");
        let mut b = SeededIds::new(b"x");
        assert_eq!(a.trace_id(), b.trace_id());
        assert_eq!(a.span_id(), b.span_id());
        let mut r1 = RandomIds::default();
        let mut r2 = RandomIds::default();
        assert_ne!(r1.trace_id(), r2.trace_id());
        assert_eq!(hex(&[0x0a, 0xff]), "0aff");
    }
}
