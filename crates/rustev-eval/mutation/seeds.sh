#!/bin/sh
# Spec 004 acceptance: seeded defects in scope isolation, expiry, output
# equivalence, fallback identity, capture completeness, denominator
# accounting and split leakage must each be detected by the tests. Works on
# a temporary copy; see seeds.py.
set -eu
exec python3 "$(dirname "$0")/seeds.py" "$@"
