//! Numeric versions and comparator sets (spec 034, 3.6.1 and 3.6.2).
use std::{cmp::Ordering, fmt, str::FromStr};

/// Decimal components are kept as digits: the grammar has no machine-word
/// ceiling. Length followed by lexical comparison gives numeric ordering.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Version([String; 3]);
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionError {
    InvalidVersion,
    InvalidRange,
    ComparatorCount,
}
impl fmt::Display for VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidVersion => "expected MAJOR.MINOR.PATCH without leading zeros or suffixes",
            Self::InvalidRange => "expected comparators separated by single spaces",
            Self::ComparatorCount => "expected one to four comparators",
        })
    }
}
impl std::error::Error for VersionError {}
impl FromStr for Version {
    type Err = VersionError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.split('.').collect();
        if parts.len() != 3
            || parts.iter().any(|p| {
                p.is_empty()
                    || !p.bytes().all(|b| b.is_ascii_digit())
                    || (p.len() > 1 && p.starts_with('0'))
            })
        {
            return Err(VersionError::InvalidVersion);
        }
        Ok(Self([parts[0].into(), parts[1].into(), parts[2].into()]))
    }
}
impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .iter()
            .zip(&other.0)
            .map(|(a, b)| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
            .find(|c| *c != Ordering::Equal)
            .unwrap_or(Ordering::Equal)
    }
}
impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0[0], self.0[1], self.0[2])
    }
}

/// A conjunction, retaining its original spelling for display (3.6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRange {
    text: String,
    comparators: Vec<(Comparison, Version)>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Comparison {
    Ge,
    Gt,
    Le,
    Lt,
    Eq,
}
impl FromStr for VersionRange {
    type Err = VersionError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<_> = s.split(' ').collect();
        if parts.iter().any(|p| p.is_empty()) {
            return Err(VersionError::InvalidRange);
        }
        if parts.len() > 4 {
            return Err(VersionError::ComparatorCount);
        }
        let mut comparators = Vec::new();
        for p in parts {
            let (op, v) = [
                (">=", Comparison::Ge),
                ("<=", Comparison::Le),
                (">", Comparison::Gt),
                ("<", Comparison::Lt),
                ("=", Comparison::Eq),
            ]
            .into_iter()
            .find_map(|(prefix, op)| p.strip_prefix(prefix).map(|v| (op, v)))
            .unwrap_or((Comparison::Eq, p));
            comparators.push((op, v.parse()?));
        }
        Ok(Self {
            text: s.into(),
            comparators,
        })
    }
}
impl VersionRange {
    pub fn satisfies(&self, version: &Version) -> bool {
        self.comparators.iter().all(|(op, v)| match op {
            Comparison::Ge => version >= v,
            Comparison::Gt => version > v,
            Comparison::Le => version <= v,
            Comparison::Lt => version < v,
            Comparison::Eq => version == v,
        })
    }
}
impl fmt::Display for VersionRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}
