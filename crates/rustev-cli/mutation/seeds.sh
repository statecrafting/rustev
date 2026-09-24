#!/bin/sh
# Spec 006 acceptance: seeded defects in scope flag handling, byte caps,
# no-clobber, exit-code mapping, store confinement, adapter correctness and
# sink acknowledgement must each be detected by the tests. Works on a
# temporary copy; see seeds.py.
set -eu
exec python3 "$(dirname "$0")/seeds.py" "$@"
