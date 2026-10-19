//! Evidence quality at compile time (spec 032). This change carries the
//! bounds of version 2 types and the structure of a presence predicate; the
//! rest of compile-time evidence quality follows.

use rustev_contract::grade::PresentWhen;

/// Bounds of a presence predicate (spec 032, 3.4.2).
pub const PRESENT_WHEN_MAX_DEPTH: usize = 4;
pub const PRESENT_WHEN_MAX_LEAVES: usize = 32;

/// Non-empty lists, at most 4 deep, at most 32 leaves.
pub(crate) fn present_when_structure(p: &PresentWhen) -> Result<(), String> {
    fn lists(p: &PresentWhen) -> Result<(), String> {
        match p {
            PresentWhen::AnyNonNull(f) | PresentWhen::AllNonNull(f) if f.is_empty() => {
                Err("a presence predicate names no field".into())
            }
            PresentWhen::All(ps) | PresentWhen::Any(ps) if ps.is_empty() => {
                Err("a presence predicate combines nothing".into())
            }
            PresentWhen::All(ps) | PresentWhen::Any(ps) => ps.iter().try_for_each(lists),
            _ => Ok(()),
        }
    }
    lists(p)?;
    let (depth, leaves) = p.shape();
    if depth > PRESENT_WHEN_MAX_DEPTH || leaves > PRESENT_WHEN_MAX_LEAVES {
        return Err(format!(
            "a presence predicate is {depth} deep with {leaves} leaves; at most \
             {PRESENT_WHEN_MAX_DEPTH} and {PRESENT_WHEN_MAX_LEAVES}"
        ));
    }
    Ok(())
}
