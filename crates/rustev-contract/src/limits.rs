//! The named parse limit sets of spec 002, 3.2.5. Each document type uses
//! exactly one of them through [`crate::Document::LIMITS`].

use crate::bounded::ParseLimits;

const KIB: usize = 1024;
const MIB: usize = 1024 * KIB;

/// Decision definitions.
pub const DEFINITION_V1: ParseLimits = ParseLimits {
    max_bytes: MIB,
    max_depth: 32,
    max_string_bytes: 64 * KIB,
    max_collection_len: 4096,
    max_total_values: 100_000,
    max_number_bytes: 40,
    allow_fractional_numbers: false,
};

/// Context snapshots.
pub const SNAPSHOT_V1: ParseLimits = ParseLimits {
    max_bytes: 4 * MIB,
    max_depth: 32,
    max_string_bytes: 256 * KIB,
    max_collection_len: 10_000,
    max_total_values: 500_000,
    max_number_bytes: 40,
    allow_fractional_numbers: false,
};

/// Backend descriptors and calibration artifacts.
pub const DESCRIPTOR_V1: ParseLimits = ParseLimits {
    max_bytes: 256 * KIB,
    max_depth: 16,
    max_string_bytes: 4 * KIB,
    max_collection_len: 1024,
    max_total_values: 20_000,
    max_number_bytes: 40,
    allow_fractional_numbers: false,
};

/// Backend outputs; masses, logits and scores are JSON numbers.
pub const BACKEND_OUTPUT_V1: ParseLimits = ParseLimits {
    max_bytes: MIB,
    max_depth: 8,
    max_string_bytes: 4 * KIB,
    max_collection_len: 4096,
    max_total_values: 50_000,
    max_number_bytes: 40,
    allow_fractional_numbers: true,
};

/// Compiled plans.
pub const PLAN_V1: ParseLimits = ParseLimits {
    max_bytes: 2 * MIB,
    max_depth: 32,
    max_string_bytes: 64 * KIB,
    max_collection_len: 4096,
    max_total_values: 200_000,
    max_number_bytes: 40,
    allow_fractional_numbers: false,
};

/// Judgments, evidence records and evaluation reports.
pub const RECORD_V1: ParseLimits = ParseLimits {
    max_bytes: 4 * MIB,
    max_depth: 32,
    max_string_bytes: 256 * KIB,
    max_collection_len: 10_000,
    max_total_values: 500_000,
    max_number_bytes: 40,
    allow_fractional_numbers: true,
};
