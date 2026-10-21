//! `exp` and `ln` for every pure path (spec 047).
//!
//! The platform's math library differs between targets (glibc, musl, Apple,
//! Microsoft and Rust's own on WebAssembly) in the last bit of some results,
//! so the pure crates never call `f64::exp` or `f64::ln`. These functions
//! are one pure Rust implementation, the `libm` crate exact-pinned without
//! default features, and give the same bits on every target. They are not
//! correctly rounded, so they stay outside exact computation (spec 002,
//! 3.4.6); a test (`tests/platform_math.rs`) refuses a direct call.

/// e raised to `x`.
pub fn exp(x: f64) -> f64 {
    libm::exp(x)
}

/// The natural logarithm of `x`.
pub fn ln(x: f64) -> f64 {
    libm::log(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn special_values() {
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(exp(f64::NEG_INFINITY), 0.0);
        assert_eq!(exp(-746.0), 0.0);
        assert_eq!(ln(1.0), 0.0);
        assert_eq!(ln(0.0), f64::NEG_INFINITY);
        assert!(ln(-1.0).is_nan());
    }

    #[test]
    fn known_bits() {
        // Pinned results, so a pin change that moves a bit is seen here
        // before the differential corpus (spec 027, 3.11.3) runs.
        assert_eq!(exp(-1.0).to_bits(), 0x3fd7_8b56_362c_ef38);
        assert_eq!(ln(0.5).to_bits(), 0xbfe6_2e42_fefa_39ef);
        assert_eq!(ln(f64::MIN_POSITIVE).to_bits(), 0xc086_232b_dd7a_bcd2);
    }
}
